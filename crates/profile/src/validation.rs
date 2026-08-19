use std::collections::BTreeSet;
use std::net::IpAddr;
use std::path::Path;

use base64::Engine as _;
use sha2::{Digest as _, Sha256};

use crate::{ProfileBundle, ProfileError, TrustSpec};

pub(crate) fn validate_manifest(bundle: &ProfileBundle) -> Result<(), ProfileError> {
    if bundle.schema_version != 1 {
        return invalid("unsupported schema_version");
    }
    if !valid_token(&bundle.bundle_version, 64) {
        return invalid("bundle_version must be a short ASCII token");
    }
    if bundle.profiles.is_empty() || bundle.profiles.len() > 16 {
        return invalid("profiles must contain between 1 and 16 entries");
    }
    let mut ids = BTreeSet::new();
    for profile in &bundle.profiles {
        if !valid_profile_key(&profile.id) {
            return invalid("profile id is invalid");
        }
        if !ids.insert(profile.id.as_str()) {
            return invalid("profile ids must be unique");
        }
        if profile.display_name.is_empty()
            || profile.display_name.len() > 80
            || profile.display_name.chars().any(char::is_control)
        {
            return invalid("profile display_name is invalid");
        }
        if !valid_dns_name(&profile.host) {
            return invalid("profile host is invalid");
        }
        if profile.email_domains.is_empty() || profile.email_domains.len() > 16 {
            return invalid("email_domains must contain between 1 and 16 entries");
        }
        let mut domains = BTreeSet::new();
        for domain in &profile.email_domains {
            if !valid_dns_name(domain) || !domains.insert(domain.to_ascii_lowercase()) {
                return invalid("email domain is invalid or duplicated");
            }
        }
        if profile.username_realm.as_deref().is_some_and(|realm| !valid_realm(realm)) {
            return invalid("username_realm is invalid");
        }
        if !matches!(profile.device_id_length, 16 | 32) {
            return invalid("device_id_length must be 16 or 32");
        }
        validate_trust(&profile.trust)?;
    }
    Ok(())
}

fn validate_trust(trust: &TrustSpec) -> Result<(), ProfileError> {
    match trust {
        TrustSpec::System => Ok(()),
        TrustSpec::ExclusivePem { pem, sha256 } => {
            if pem.len() > 64 * 1024 {
                return trust_error("PEM certificate is too large");
            }
            let actual = certificate_fingerprint(pem.as_bytes())?;
            let expected = normalize_fingerprint(sha256)?;
            if actual != expected {
                return trust_error("PEM fingerprint does not match sha256");
            }
            Ok(())
        }
    }
}

/// Returns the canonical uppercase SHA-256 fingerprint of one PEM certificate.
pub fn certificate_fingerprint(pem: &[u8]) -> Result<String, ProfileError> {
    let text =
        std::str::from_utf8(pem).map_err(|_| ProfileError::Trust("PEM must be UTF-8".into()))?;
    if text.contains("PRIVATE KEY") {
        return trust_error("PEM must not contain a private key");
    }
    let begin = "-----BEGIN CERTIFICATE-----";
    let end = "-----END CERTIFICATE-----";
    if text.matches(begin).count() != 1 || text.matches(end).count() != 1 {
        return trust_error("PEM must contain exactly one certificate");
    }
    let encoded = text
        .split_once(begin)
        .and_then(|(_, rest)| rest.split_once(end).map(|(body, _)| body))
        .ok_or_else(|| ProfileError::Trust("PEM certificate markers are malformed".into()))?
        .lines()
        .map(str::trim)
        .collect::<String>();
    let der = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| ProfileError::Trust("PEM certificate base64 is invalid".into()))?;
    if der.is_empty() {
        return trust_error("PEM certificate is empty");
    }
    Ok(Sha256::digest(der).iter().map(|byte| format!("{byte:02X}")).collect())
}

/// Normalizes a colon-delimited or compact SHA-256 fingerprint.
pub fn normalize_fingerprint(value: &str) -> Result<String, ProfileError> {
    let normalized = value.chars().filter(|character| *character != ':').collect::<String>();
    if normalized.len() != 64 || !normalized.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return trust_error("sha256 must contain exactly 32 hexadecimal bytes");
    }
    Ok(normalized.to_ascii_uppercase())
}

/// Returns whether a string is a stable public profile key.
#[must_use]
pub fn valid_profile_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index > 0 && matches!(byte, b'-' | b'_'))
        })
}

fn valid_token(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_realm(value: &str) -> bool {
    valid_token(value, 64) && !value.starts_with(['.', '-', '_'])
}

fn valid_dns_name(value: &str) -> bool {
    value.len() <= 253
        && value.contains('.')
        && !value.ends_with('.')
        && value.parse::<IpAddr>().is_err()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        })
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

pub(crate) fn reject_link(path: &Path) -> Result<(), ProfileError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| ProfileError::Read)?;
    if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
        return Err(ProfileError::Read);
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
const fn is_reparse_point(_: &std::fs::Metadata) -> bool {
    false
}

fn invalid<T>(message: &str) -> Result<T, ProfileError> {
    Err(ProfileError::Invalid(message.into()))
}

fn trust_error<T>(message: &str) -> Result<T, ProfileError> {
    Err(ProfileError::Trust(message.into()))
}
