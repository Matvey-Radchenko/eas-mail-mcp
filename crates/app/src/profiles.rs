use std::path::Path;

use eas_mail_profile::{ProfileBundle, VerifiedBundle};
use eas_mail_protocol::ProfileRegistry;

use crate::platform;
use crate::{AppError, ErrorCode, Result};

/// Loads and validates a local profile bundle when it exists.
pub fn load_profile_bundle(path: &Path) -> Result<Option<VerifiedBundle>> {
    if !path.exists() {
        return Ok(None);
    }
    eas_mail_profile::load(path).map(Some).map_err(profile_error)
}

/// Loads the local profile registry when it exists.
pub fn load_profile_registry(path: &Path) -> Result<Option<ProfileRegistry>> {
    load_profile_bundle(path)?
        .map(|bundle| ProfileRegistry::from_verified(&bundle).map_err(AppError::from))
        .transpose()
}

pub(crate) fn require_profile_registry(path: &Path) -> Result<ProfileRegistry> {
    load_profile_registry(path)?.ok_or_else(|| {
        AppError::new(ErrorCode::ConfigInvalid, "no EAS endpoint profiles are configured")
            .remediation("Run eas-mail-mcp setup or profile import")
    })
}

pub(crate) fn save_profile_bundle(path: &Path, bundle: &ProfileBundle) -> Result<VerifiedBundle> {
    let document = eas_mail_profile::serialize(bundle).map_err(profile_error)?;
    let verified = eas_mail_profile::parse(&document).map_err(profile_error)?;
    platform::atomic_write(path, document.as_bytes()).map_err(|_| {
        AppError::new(ErrorCode::StorageError, "cannot save local endpoint profiles")
    })?;
    Ok(verified)
}

fn profile_error(_: eas_mail_profile::ProfileError) -> AppError {
    AppError::new(ErrorCode::ConfigInvalid, "endpoint profile configuration is invalid")
        .remediation("Validate the profile file and import it again")
}

#[cfg(test)]
pub(crate) fn example_registry() -> Result<ProfileRegistry> {
    ProfileRegistry::from_toml(include_str!("../../../profile.example.toml"))
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests;
