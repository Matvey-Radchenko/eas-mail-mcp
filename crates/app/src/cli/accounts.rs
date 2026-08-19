use std::io::BufRead as _;
use std::sync::Arc;

use eas_mail_protocol::{ProfileKey, ProfileRegistry};
use zeroize::Zeroizing;

use super::account_secrets::restore as restore_secret;
use super::account_secrets::{open as secret_store, replace as replace_secret};
use super::account_secrets::{replace_optional as replace_secret_optional, replace_password};
use super::{AccountCommand, AddAccountArgs, SetupArgs, Toggle, prompt};
use crate::backend::{AccountBackend as _, EasMailbox};
use crate::{
    AccountConfig, AccountSecret, AppError, ErrorCode, Paths, Result, SecretBundle, SecretStore,
    load_config, save_config,
};

pub(super) struct AddRequest {
    account_id: String,
    profile: ProfileKey,
    email: String,
    username: String,
    password_stdin: bool,
    write_enabled: bool,
}

pub(super) fn interactive_request(
    arguments: SetupArgs,
    profiles: &ProfileRegistry,
) -> Result<AddRequest> {
    let account_id = required(arguments.account_id, "Account ID")?;
    let profile = match arguments.profile {
        Some(value) => value,
        None => ProfileKey::new(prompt(&profile_prompt(profiles))?).map_err(AppError::from)?,
    };
    profiles.require(&profile).map_err(AppError::from)?;
    Ok(AddRequest {
        account_id,
        profile,
        email: required(arguments.email, "Email")?,
        username: required(arguments.username, "Username")?,
        password_stdin: arguments.password_stdin,
        write_enabled: arguments.enable_writes,
    })
}

pub(super) async fn run(
    paths: &Paths,
    command: AccountCommand,
    profiles: Option<&ProfileRegistry>,
) -> Result<serde_json::Value> {
    match command {
        AccountCommand::List => list(paths),
        AccountCommand::Add(arguments) => {
            add(paths, request(arguments), require_profiles(profiles)?).await
        }
        AccountCommand::UpdatePassword(arguments) => {
            update_password(
                paths,
                &arguments.account_id,
                arguments.password_stdin,
                require_profiles(profiles)?,
            )
            .await
        }
        AccountCommand::SetWrites(arguments) => {
            set_writes(paths, &arguments.account_id, matches!(arguments.value, Toggle::On))
        }
        AccountCommand::Remove(arguments) => remove(paths, &arguments.account_id),
    }
}

pub(super) async fn add(
    paths: &Paths,
    request: AddRequest,
    profiles: &ProfileRegistry,
) -> Result<serde_json::Value> {
    let mut config = load_config(&paths.config)?;
    if config.accounts.contains_key(&request.account_id) {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "account identifier already exists",
        ));
    }
    let account = AccountConfig {
        profile: request.profile.clone(),
        email: request.email,
        username: request.username,
        enabled: true,
        write_enabled: request.write_enabled,
    };
    account.validate(profiles)?;
    let password = read_password(request.password_stdin)?;
    validate_password(&password)?;
    let store = secret_store(paths);
    let profile = profiles.require(&request.profile).map_err(AppError::from)?;
    let candidate = AccountSecret {
        password: password.to_string(),
        device_id: SecretBundle::device_id(profile.device_id_length())?,
        policy_key: 0,
        policy: None,
    };
    let original = replace_secret(&store, &request.account_id, candidate.clone())?;
    let verification = verify(&request.account_id, &account, Arc::clone(&store), profiles).await;
    let folders = match verification {
        Ok(value) => value,
        Err(error) => {
            restore_secret(&store, &request.account_id, Some(&candidate), original.as_ref())?;
            return Err(error);
        }
    };
    config.accounts.insert(request.account_id.clone(), account);
    if let Err(error) = save_config(&paths.config, &config) {
        restore_secret(&store, &request.account_id, Some(&candidate), original.as_ref())?;
        return Err(error);
    }
    Ok(serde_json::json!({
        "account_id": request.account_id,
        "configured": true,
        "write_enabled": request.write_enabled,
        "folders_verified": folders,
    }))
}

async fn update_password(
    paths: &Paths,
    account_id: &str,
    password_stdin: bool,
    profiles: &ProfileRegistry,
) -> Result<serde_json::Value> {
    let config = load_config(&paths.config)?;
    let account = config.accounts.get(account_id).cloned().ok_or_else(|| {
        AppError::new(ErrorCode::NotFound, "account is not configured").account(account_id)
    })?;
    let password = read_password(password_stdin)?;
    validate_password(&password)?;
    let store = secret_store(paths);
    let (original, candidate) = replace_password(&store, account_id, &password)?;
    let verification = verify(account_id, &account, Arc::clone(&store), profiles).await;
    match verification {
        Ok(folders) => Ok(serde_json::json!({
            "account_id": account_id,
            "password_updated": true,
            "folders_verified": folders,
        })),
        Err(error) => {
            restore_secret(&store, account_id, Some(&candidate), Some(&original))?;
            Err(error)
        }
    }
}

async fn verify(
    account_id: &str,
    account: &AccountConfig,
    store: Arc<dyn SecretStore>,
    profiles: &ProfileRegistry,
) -> Result<usize> {
    let mailbox = EasMailbox::production(account_id.to_owned(), account.clone(), store, profiles)?;
    mailbox.folders().await.map(|folders| folders.len())
}

fn list(paths: &Paths) -> Result<serde_json::Value> {
    let config = load_config(&paths.config)?;
    let accounts = config
        .accounts
        .into_iter()
        .map(|(account_id, account)| {
            serde_json::json!({
                "account_id": account_id,
                "profile": account.profile.as_str(),
                "email": account.email,
                "enabled": account.enabled,
                "write_enabled": account.write_enabled,
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({ "accounts": accounts }))
}

fn set_writes(paths: &Paths, account_id: &str, enabled: bool) -> Result<serde_json::Value> {
    let mut config = load_config(&paths.config)?;
    let account = config.accounts.get_mut(account_id).ok_or_else(|| {
        AppError::new(ErrorCode::NotFound, "account is not configured").account(account_id)
    })?;
    account.write_enabled = enabled;
    save_config(&paths.config, &config)?;
    Ok(serde_json::json!({ "account_id": account_id, "write_enabled": enabled }))
}

fn remove(paths: &Paths, account_id: &str) -> Result<serde_json::Value> {
    let mut config = load_config(&paths.config)?;
    if config.accounts.remove(account_id).is_none() {
        return Err(
            AppError::new(ErrorCode::NotFound, "account is not configured").account(account_id)
        );
    }
    let store = secret_store(paths);
    let original = replace_secret_optional(&store, account_id, None)?;
    if let Err(error) = save_config(&paths.config, &config) {
        restore_secret(&store, account_id, None, original.as_ref())?;
        return Err(error);
    }
    Ok(serde_json::json!({ "account_id": account_id, "removed": true }))
}

fn request(arguments: AddAccountArgs) -> AddRequest {
    AddRequest {
        account_id: arguments.account_id,
        profile: arguments.profile,
        email: arguments.email,
        username: arguments.username,
        password_stdin: arguments.password_stdin,
        write_enabled: arguments.enable_writes,
    }
}

fn profile_prompt(profiles: &ProfileRegistry) -> String {
    let keys =
        profiles.profiles().iter().map(|profile| profile.key()).collect::<Vec<_>>().join("/");
    format!("Profile ({keys})")
}

fn require_profiles(profiles: Option<&ProfileRegistry>) -> Result<&ProfileRegistry> {
    profiles.ok_or_else(|| {
        AppError::new(ErrorCode::ConfigInvalid, "no EAS endpoint profiles are configured")
            .remediation("Run eas-mail-mcp setup or profile import")
    })
}

fn required(value: Option<String>, label: &str) -> Result<String> {
    match value {
        Some(value) if !value.trim().is_empty() => Ok(value),
        Some(_) => Err(AppError::new(ErrorCode::ValidationFailed, "required value is empty")),
        None => prompt(label),
    }
}

fn read_password(from_stdin: bool) -> Result<Zeroizing<String>> {
    let value = if from_stdin {
        let mut value = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut value)
            .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot read password"))?;
        value.trim_end_matches(['\r', '\n']).to_owned()
    } else {
        rpassword::prompt_password("Exchange password: ")
            .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot read password"))?
    };
    Ok(Zeroizing::new(value))
}

fn validate_password(password: &str) -> Result<()> {
    if password.is_empty() || password.contains(['\0', '\r', '\n']) {
        Err(AppError::new(ErrorCode::ValidationFailed, "password is empty or malformed"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
