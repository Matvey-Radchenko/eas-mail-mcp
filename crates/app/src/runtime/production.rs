use crate::backend::{AccountBackend, BackendAccount, EasMailbox, UnavailableBackend};
use crate::keychain::AccountSecret;
use crate::{AccountConfig, AppError, ErrorCode, SecretStore};
use eas_mail_protocol::ProfileRegistry;
use std::sync::Arc;

pub(super) fn configured_backend(
    account_id: String,
    config: AccountConfig,
    secrets: Arc<dyn SecretStore>,
    secret: Option<AccountSecret>,
    profiles: &ProfileRegistry,
) -> Arc<dyn AccountBackend> {
    let metadata = BackendAccount {
        account_id: account_id.clone(),
        profile: config.profile.clone(),
        email: config.email.clone(),
        email_domains: profiles
            .require(&config.profile)
            .map(|profile| profile.email_domains().to_vec())
            .unwrap_or_default(),
        enabled: config.enabled,
        write_enabled: config.write_enabled,
    };
    let backend = if !config.enabled {
        Err(AppError::new(ErrorCode::ConfigInvalid, "account is disabled in local configuration"))
    } else {
        config.validate(profiles).and_then(|()| {
            let secret = secret.ok_or_else(|| {
                AppError::new(ErrorCode::AuthRequired, "account credentials are missing")
            })?;
            EasMailbox::production_with_secret(
                account_id.clone(),
                config,
                secrets,
                secret,
                profiles,
            )
        })
    };
    match backend {
        Ok(backend) => Arc::new(backend),
        Err(error) => Arc::new(UnavailableBackend::new(metadata, error.account(account_id))),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod upgrade;
