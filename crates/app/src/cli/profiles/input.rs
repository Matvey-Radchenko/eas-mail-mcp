use std::fs;
use std::path::Path;

use eas_mail_profile::{ProfileSpec, TrustSpec};
use eas_mail_protocol::ProfileKey;

use super::super::{ProfileAddArgs, prompt};
use super::profile_error;
use crate::platform;
use crate::{AppError, ErrorCode, Result};

pub(super) fn interactive_profile(arguments: ProfileAddArgs) -> Result<ProfileSpec> {
    let interactive = arguments.id.is_none()
        && arguments.display_name.is_none()
        && arguments.host.is_none()
        && arguments.email_domains.is_empty()
        && arguments.device_id_length.is_none()
        && arguments.pem.is_none();
    let id = required(arguments.id, "Profile ID")?;
    ProfileKey::new(id.clone()).map_err(AppError::from)?;
    let display_name = required(arguments.display_name, "Display name")?;
    let host = required(arguments.host, "Exchange host")?.to_ascii_lowercase();
    let email_domains = domains(arguments.email_domains)?;
    let username_realm = match arguments.username_realm {
        Some(value) => Some(value),
        None if interactive => optional(prompt("Username realm (optional)")?),
        None => None,
    };
    let device_id_length = device_id_length(arguments.device_id_length, interactive)?;
    let pem = match arguments.pem {
        Some(path) => Some(path),
        None if interactive => optional(prompt("Exclusive PEM path (optional)")?).map(Into::into),
        None => None,
    };
    Ok(ProfileSpec {
        id,
        display_name,
        host,
        email_domains,
        username_realm,
        device_id_length,
        trust: trust(pem.as_deref())?,
    })
}

fn domains(values: Vec<String>) -> Result<Vec<String>> {
    let values = if values.is_empty() {
        prompt("Email domains (comma-separated)")?.split(',').map(str::to_owned).collect()
    } else {
        values
    };
    Ok(values
        .into_iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect())
}

fn device_id_length(value: Option<u8>, interactive: bool) -> Result<u8> {
    match value {
        Some(value) => Ok(value),
        None if interactive => optional(prompt("Device ID length [16]")?)
            .map_or(Ok(16), |value| value.parse::<u8>().map_err(|_| invalid_value())),
        None => Ok(16),
    }
}

fn trust(path: Option<&Path>) -> Result<TrustSpec> {
    let Some(path) = path else { return Ok(TrustSpec::System) };
    platform::reject_existing_link(path)
        .map_err(|_| AppError::new(ErrorCode::ConfigInvalid, "PEM path is not safe"))?;
    let bytes = fs::read(path)
        .map_err(|_| AppError::new(ErrorCode::ConfigInvalid, "PEM certificate cannot be read"))?;
    let sha256 = eas_mail_profile::certificate_fingerprint(&bytes).map_err(profile_error)?;
    let pem = String::from_utf8(bytes)
        .map_err(|_| AppError::new(ErrorCode::ConfigInvalid, "PEM certificate is not UTF-8"))?;
    Ok(TrustSpec::ExclusivePem { pem, sha256 })
}

fn required(value: Option<String>, label: &str) -> Result<String> {
    let value = value.map_or_else(|| prompt(label), Ok)?;
    if value.trim().is_empty() {
        Err(AppError::new(ErrorCode::ValidationFailed, "required value is empty"))
    } else {
        Ok(value)
    }
}

fn optional(value: String) -> Option<String> {
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn invalid_value() -> AppError {
    AppError::new(ErrorCode::ValidationFailed, "interactive profile value is invalid")
}

#[cfg(test)]
mod tests {
    use super::{device_id_length, domains, optional, required};

    #[test]
    fn explicit_values_are_normalized_and_empty_required_values_fail() -> anyhow::Result<()> {
        assert_eq!(domains(vec![" Example.Invalid ".into(), "".into()])?, ["example.invalid"]);
        assert_eq!(device_id_length(Some(32), false)?, 32);
        assert_eq!(device_id_length(None, false)?, 16);
        assert_eq!(optional(" value ".into()).as_deref(), Some("value"));
        assert!(optional("  ".into()).is_none());
        assert_eq!(required(Some("value".into()), "unused")?, "value");
        assert!(required(Some("  ".into()), "unused").is_err());
        Ok(())
    }
}
