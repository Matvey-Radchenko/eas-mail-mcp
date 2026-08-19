use std::fmt;
use std::str::FromStr;

use eas_mail_profile::{TrustSpec, VerifiedBundle, valid_profile_key};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{EasError, Result};

/// Validated identifier of a locally configured endpoint profile.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileKey(String);

impl Default for ProfileKey {
    fn default() -> Self {
        Self("default".into())
    }
}

impl ProfileKey {
    /// Parses a stable lowercase profile identifier.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if valid_profile_key(&value) {
            Ok(Self(value))
        } else {
            Err(EasError::InvalidConfiguration("profile key is invalid".into()))
        }
    }

    /// Returns the serialized profile identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProfileKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ProfileKey {
    type Err = EasError;

    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

impl Serialize for ProfileKey {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProfileKey {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Validated Exchange endpoint profile loaded from local configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    key: ProfileKey,
    display_name: String,
    pub(crate) host: String,
    email_domains: Vec<String>,
    username_realm: Option<String>,
    device_id_length: u8,
    pub(crate) extra_ca_pem: Option<Vec<u8>>,
}

impl Profile {
    /// Returns this profile's stable identifier.
    #[must_use]
    pub fn key(&self) -> &str {
        self.key.as_str()
    }

    /// Returns this profile's human-readable name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns the only allowed EAS URL for this profile.
    #[must_use]
    pub fn endpoint(&self) -> String {
        format!("https://{}/Microsoft-Server-ActiveSync", self.host)
    }

    /// Returns the exact Device ID length required by this endpoint.
    #[must_use]
    pub const fn device_id_length(&self) -> usize {
        self.device_id_length as usize
    }

    /// Reports whether this profile uses an exclusive local trust anchor.
    #[must_use]
    pub const fn has_extra_trust_anchor(&self) -> bool {
        self.extra_ca_pem.is_some()
    }

    /// Validates account identity against the local profile.
    pub fn validate_identity(&self, email: &str, username: &str) -> Result<()> {
        let domain = email.rsplit_once('@').map(|(_, domain)| domain);
        if domain.is_none_or(|domain| {
            !self.email_domains.iter().any(|allowed| domain.eq_ignore_ascii_case(allowed))
        }) {
            return Err(EasError::InvalidConfiguration(
                "email does not match the selected profile".into(),
            ));
        }
        if username.trim().is_empty() || username.chars().any(char::is_control) {
            return Err(EasError::InvalidConfiguration("username must not be empty".into()));
        }
        if let Some(required) = &self.username_realm {
            let actual = username.split_once('\\').map(|(realm, _)| realm);
            if actual.is_none_or(|realm| !realm.eq_ignore_ascii_case(required)) {
                return Err(EasError::InvalidConfiguration(
                    "username does not match the selected profile realm".into(),
                ));
            }
        }
        Ok(())
    }

    /// Validates the stable EAS Device ID for this profile.
    pub fn validate_device_id(&self, device_id: &str) -> Result<()> {
        if device_id.len() != self.device_id_length()
            || !device_id.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(EasError::InvalidConfiguration(
                "DeviceId does not match the selected profile".into(),
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn localhost() -> Self {
        Self {
            key: ProfileKey("localhost".into()),
            display_name: "Local test".into(),
            host: "localhost".into(),
            email_domains: vec!["example.invalid".into()],
            username_realm: None,
            device_id_length: 32,
            extra_ca_pem: None,
        }
    }
}

/// Registry of validated profiles loaded for one process.
#[derive(Debug, Clone)]
pub struct ProfileRegistry {
    bundle_version: String,
    bundle_hash: String,
    profiles: Vec<Profile>,
}

impl ProfileRegistry {
    /// Parses and validates a registry from portable runtime TOML.
    pub fn from_toml(input: &str) -> Result<Self> {
        let bundle = eas_mail_profile::parse(input)
            .map_err(|_| EasError::InvalidConfiguration("profile TOML is invalid".into()))?;
        Self::from_verified(&bundle)
    }

    /// Constructs a registry from a validated runtime bundle.
    pub fn from_verified(bundle: &VerifiedBundle) -> Result<Self> {
        let mut profiles = Vec::with_capacity(bundle.manifest.profiles.len());
        for spec in &bundle.manifest.profiles {
            let key = ProfileKey::new(spec.id.clone())?;
            let extra_ca_pem = match &spec.trust {
                TrustSpec::System => None,
                TrustSpec::ExclusivePem { pem, .. } => Some(pem.as_bytes().to_vec()),
            };
            profiles.push(Profile {
                key,
                display_name: spec.display_name.clone(),
                host: spec.host.clone(),
                email_domains: spec.email_domains.clone(),
                username_realm: spec.username_realm.clone(),
                device_id_length: spec.device_id_length,
                extra_ca_pem,
            });
        }
        Ok(Self {
            bundle_version: bundle.manifest.bundle_version.clone(),
            bundle_hash: bundle.hash.clone(),
            profiles,
        })
    }

    /// Resolves a profile key without permitting an arbitrary endpoint.
    #[must_use]
    pub fn get(&self, key: &ProfileKey) -> Option<&Profile> {
        self.profiles.iter().find(|profile| profile.key == *key)
    }

    /// Resolves a profile or returns a redacted configuration error.
    pub fn require(&self, key: &ProfileKey) -> Result<&Profile> {
        self.get(key)
            .ok_or_else(|| EasError::InvalidConfiguration("profile is not configured".into()))
    }

    /// Returns all available profiles.
    #[must_use]
    pub fn profiles(&self) -> &[Profile] {
        &self.profiles
    }

    /// Returns the first profile key for deterministic harness defaults.
    #[must_use]
    pub fn default_key(&self) -> Option<ProfileKey> {
        self.profiles.first().map(|profile| profile.key.clone())
    }

    /// Returns the local profile bundle version.
    #[must_use]
    pub fn bundle_version(&self) -> &str {
        &self.bundle_version
    }

    /// Returns the SHA-256 hash of the profile file.
    #[must_use]
    pub fn bundle_hash(&self) -> &str {
        &self.bundle_hash
    }

    /// Reports whether no profiles are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}
