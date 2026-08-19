mod input;

use std::fs;
use std::path::Path;

use eas_mail_profile::{ProfileBundle, TrustSpec, VerifiedBundle};
use eas_mail_protocol::{ProfileKey, ProfileRegistry};

use super::{
    ProfileAddArgs, ProfileCommand, ProfileExportArgs, ProfileImportArgs, ProfileRemoveArgs,
    ProfileValidateArgs, confirm, prompt,
};
use crate::platform;
use crate::profiles::save_profile_bundle;
use crate::{AppError, ErrorCode, Paths, Result, load_config, load_profile_bundle};

use self::input::interactive_profile;

pub(super) fn run(paths: &Paths, command: ProfileCommand) -> Result<serde_json::Value> {
    match command {
        ProfileCommand::Import(arguments) => import(paths, arguments),
        ProfileCommand::Add(arguments) => add(paths, arguments),
        ProfileCommand::Validate(arguments) => validate(paths, arguments),
        ProfileCommand::List => list(paths),
        ProfileCommand::Export(arguments) => export(paths, arguments),
        ProfileCommand::Remove(arguments) => remove(paths, arguments),
    }
}

pub(super) fn ensure_for_setup(
    paths: &Paths,
    import_file: Option<&Path>,
) -> Result<(ProfileRegistry, serde_json::Value)> {
    if let Some(file) = import_file {
        let result =
            import(paths, ProfileImportArgs { file: file.to_owned(), replace: false, yes: false })?;
        let registry = crate::profiles::require_profile_registry(&paths.profiles)?;
        return Ok((registry, result));
    }
    if let Some(bundle) = load_profile_bundle(&paths.profiles)? {
        let registry = ProfileRegistry::from_verified(&bundle).map_err(AppError::from)?;
        return Ok((registry, summary(&bundle, "reused", bundle.manifest.profiles.len())));
    }
    let result = match prompt("Profile source (import/manual)")?.to_ascii_lowercase().as_str() {
        "import" | "i" => {
            let file = Path::new(&prompt("Profile TOML path")?).to_owned();
            import(paths, ProfileImportArgs { file, replace: false, yes: false })?
        }
        "manual" | "m" => add(
            paths,
            ProfileAddArgs {
                id: None,
                display_name: None,
                host: None,
                email_domains: Vec::new(),
                username_realm: None,
                device_id_length: None,
                pem: None,
            },
        )?,
        _ => {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                "profile source must be import or manual",
            ));
        }
    };
    let registry = crate::profiles::require_profile_registry(&paths.profiles)?;
    Ok((registry, result))
}

fn import(paths: &Paths, arguments: ProfileImportArgs) -> Result<serde_json::Value> {
    let incoming = load_external(&arguments.file)?;
    let current = load_profile_bundle(&paths.profiles)?;
    let (candidate, conflicts, changed_lengths) =
        merge(current.as_ref(), &incoming, arguments.replace)?;
    if !conflicts.is_empty() && !arguments.yes && !confirm("Replace conflicting profiles")? {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "profile replacement was cancelled",
        ));
    }
    validate_accounts(paths, &candidate, &changed_lengths)?;
    let verified = save_profile_bundle(&paths.profiles, &candidate)?;
    Ok(summary(&verified, "imported", incoming.manifest.profiles.len()))
}

fn add(paths: &Paths, arguments: ProfileAddArgs) -> Result<serde_json::Value> {
    let profile = interactive_profile(arguments)?;
    let current = load_profile_bundle(&paths.profiles)?;
    if current
        .as_ref()
        .is_some_and(|bundle| bundle.manifest.profiles.iter().any(|value| value.id == profile.id))
    {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "profile identifier already exists",
        ));
    }
    let mut manifest = current.map_or_else(empty_bundle, |bundle| bundle.manifest);
    manifest.profiles.push(profile);
    let verified = save_profile_bundle(&paths.profiles, &manifest)?;
    Ok(summary(&verified, "added", 1))
}

fn validate(paths: &Paths, arguments: ProfileValidateArgs) -> Result<serde_json::Value> {
    let path = arguments.file.as_deref().unwrap_or(&paths.profiles);
    let bundle = load_external(path)?;
    Ok(summary(&bundle, "validated", bundle.manifest.profiles.len()))
}

fn list(paths: &Paths) -> Result<serde_json::Value> {
    let Some(bundle) = load_profile_bundle(&paths.profiles)? else {
        return Ok(serde_json::json!({ "profiles": [] }));
    };
    let profiles = bundle
        .manifest
        .profiles
        .iter()
        .map(|profile| {
            serde_json::json!({
                "id": profile.id,
                "display_name": profile.display_name,
                "host": profile.host,
                "email_domains": profile.email_domains,
                "username_realm": profile.username_realm,
                "device_id_length": profile.device_id_length,
                "trust": trust_name(&profile.trust),
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({ "profiles": profiles, "sha256": bundle.hash }))
}

fn export(paths: &Paths, arguments: ProfileExportArgs) -> Result<serde_json::Value> {
    let bundle = load_profile_bundle(&paths.profiles)?.ok_or_else(no_profiles)?;
    let manifest = selected_export(&bundle.manifest, arguments.id.as_ref())?;
    let document = eas_mail_profile::serialize(&manifest).map_err(profile_error)?;
    platform::atomic_write_in_existing_directory(&arguments.file, document.as_bytes())
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot export endpoint profiles"))?;
    Ok(serde_json::json!({
        "exported": manifest.profiles.len(),
        "path": arguments.file,
    }))
}

fn remove(paths: &Paths, arguments: ProfileRemoveArgs) -> Result<serde_json::Value> {
    let bundle = load_profile_bundle(&paths.profiles)?.ok_or_else(no_profiles)?;
    let config = load_config(&paths.config)?;
    if config.accounts.values().any(|account| account.profile == arguments.id) {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "profile is still referenced by an account",
        ));
    }
    if !bundle.manifest.profiles.iter().any(|profile| profile.id == arguments.id.as_str()) {
        return Err(AppError::new(ErrorCode::NotFound, "profile is not configured"));
    }
    if !arguments.yes && !confirm("Remove the selected profile")? {
        return Err(AppError::new(ErrorCode::ValidationFailed, "profile removal was cancelled"));
    }
    let mut manifest = bundle.manifest;
    manifest.profiles.retain(|profile| profile.id != arguments.id.as_str());
    if manifest.profiles.is_empty() {
        platform::reject_existing_link(&paths.profiles)
            .and_then(|()| fs::remove_file(&paths.profiles))
            .map_err(|_| {
                AppError::new(ErrorCode::StorageError, "cannot remove endpoint profile")
            })?;
    } else {
        save_profile_bundle(&paths.profiles, &manifest)?;
    }
    Ok(serde_json::json!({ "removed": arguments.id.as_str() }))
}

fn merge(
    current: Option<&VerifiedBundle>,
    incoming: &VerifiedBundle,
    replace: bool,
) -> Result<(ProfileBundle, Vec<String>, Vec<String>)> {
    let mut candidate = current.map_or_else(empty_bundle, |bundle| bundle.manifest.clone());
    if current.is_none() {
        candidate.bundle_version.clone_from(&incoming.manifest.bundle_version);
    }
    let mut conflicts = Vec::new();
    let mut changed_lengths = Vec::new();
    for profile in &incoming.manifest.profiles {
        match candidate.profiles.iter_mut().find(|value| value.id == profile.id) {
            Some(existing) if existing == profile => {}
            Some(existing) if replace => {
                conflicts.push(profile.id.clone());
                if existing.device_id_length != profile.device_id_length {
                    changed_lengths.push(profile.id.clone());
                }
                existing.clone_from(profile);
            }
            Some(_) => {
                return Err(AppError::new(
                    ErrorCode::ValidationFailed,
                    "profile identifier conflicts with local configuration",
                ));
            }
            None => candidate.profiles.push(profile.clone()),
        }
    }
    Ok((candidate, conflicts, changed_lengths))
}

fn validate_accounts(paths: &Paths, manifest: &ProfileBundle, changed: &[String]) -> Result<()> {
    let document = eas_mail_profile::serialize(manifest).map_err(profile_error)?;
    let verified = eas_mail_profile::parse(&document).map_err(profile_error)?;
    let registry = ProfileRegistry::from_verified(&verified).map_err(AppError::from)?;
    let config = load_config(&paths.config)?;
    config.validate_profiles(&registry)?;
    if config
        .accounts
        .values()
        .any(|account| changed.iter().any(|profile| profile == account.profile.as_str()))
    {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "cannot change Device ID length for a profile used by an account",
        ));
    }
    Ok(())
}

fn selected_export(bundle: &ProfileBundle, id: Option<&ProfileKey>) -> Result<ProfileBundle> {
    let Some(id) = id else { return Ok(bundle.clone()) };
    let profile = bundle
        .profiles
        .iter()
        .find(|profile| profile.id == id.as_str())
        .cloned()
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "profile is not configured"))?;
    Ok(ProfileBundle {
        schema_version: bundle.schema_version,
        bundle_version: bundle.bundle_version.clone(),
        profiles: vec![profile],
    })
}

fn load_external(path: &Path) -> Result<VerifiedBundle> {
    eas_mail_profile::load(path).map_err(profile_error)
}

fn empty_bundle() -> ProfileBundle {
    ProfileBundle { schema_version: 1, bundle_version: "local-1".into(), profiles: Vec::new() }
}

fn summary(bundle: &VerifiedBundle, action: &str, count: usize) -> serde_json::Value {
    serde_json::json!({
        "action": action,
        "count": count,
        "profiles": bundle.manifest.profiles.len(),
        "sha256": bundle.hash,
    })
}

const fn trust_name(trust: &TrustSpec) -> &'static str {
    match trust {
        TrustSpec::System => "system",
        TrustSpec::ExclusivePem { .. } => "exclusive_pem",
    }
}

fn no_profiles() -> AppError {
    AppError::new(ErrorCode::NotFound, "no endpoint profiles are configured")
}

pub(super) fn profile_error(_: eas_mail_profile::ProfileError) -> AppError {
    AppError::new(ErrorCode::ConfigInvalid, "endpoint profile configuration is invalid")
}

#[cfg(test)]
mod tests;
