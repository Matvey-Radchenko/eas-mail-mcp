use rand::RngCore as _;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use zeroize::{Zeroize, Zeroizing};

use crate::journal::with_storage_write_lock;
use crate::{AppError, ErrorCode, Result};
use eas_mail_protocol::protocol::PolicyDecision;

const SERVICE: &str = "eas-mail-mcp";
const BUNDLE_KEY: &str = "secrets-v1";

/// Secret EAS state for one account.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, zeroize::ZeroizeOnDrop)]
pub struct AccountSecret {
    /// Exchange password.
    pub password: String,
    /// Stable 8-byte uppercase hexadecimal EAS DeviceId.
    pub device_id: String,
    /// Last acknowledged EAS policy key.
    pub policy_key: u32,
    /// Enforceable limits associated with the acknowledged policy key.
    #[serde(default)]
    pub policy: Option<StoredPolicy>,
}

/// Persisted, enforceable subset of an accepted EAS policy.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize)]
pub struct StoredPolicy {
    /// Maximum permitted attachment bytes.
    pub max_attachment_bytes: u64,
    /// Whether attachment downloads are enabled.
    pub attachments_enabled: bool,
    /// Maximum mail body bytes.
    pub body_limit: usize,
    /// Effective mail FilterType.
    pub mail_filter_type: u8,
    /// Effective calendar FilterType.
    pub calendar_filter_type: u8,
}

impl From<&PolicyDecision> for StoredPolicy {
    fn from(value: &PolicyDecision) -> Self {
        Self {
            max_attachment_bytes: value.max_attachment_bytes,
            attachments_enabled: value.attachments_enabled,
            body_limit: value.body_limit,
            mail_filter_type: value.mail_filter_type,
            calendar_filter_type: value.calendar_filter_type,
        }
    }
}

impl From<&StoredPolicy> for PolicyDecision {
    fn from(value: &StoredPolicy) -> Self {
        Self {
            supported: true,
            reasons: Vec::new(),
            max_attachment_bytes: value.max_attachment_bytes,
            attachments_enabled: value.attachments_enabled,
            body_limit: value.body_limit,
            mail_filter_type: value.mail_filter_type,
            calendar_filter_type: value.calendar_filter_type,
        }
    }
}

/// Single versioned operating-system credential value used by the application.
#[derive(Clone, Serialize, Deserialize)]
pub struct SecretBundle {
    /// Secret schema version.
    pub version: u8,
    /// HMAC key used for operation payload fingerprints.
    pub hmac_key: Vec<u8>,
    /// Secret state keyed by account identifier.
    pub accounts: BTreeMap<String, AccountSecret>,
}

impl Zeroize for SecretBundle {
    fn zeroize(&mut self) {
        self.version.zeroize();
        self.hmac_key.zeroize();
        for secret in self.accounts.values_mut() {
            secret.zeroize();
        }
        self.accounts.clear();
    }
}

impl Drop for SecretBundle {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl SecretBundle {
    /// Creates an empty version-one bundle with a random HMAC key.
    #[must_use]
    pub fn new() -> Self {
        let mut hmac_key = vec![0_u8; 32];
        rand::rng().fill_bytes(&mut hmac_key);
        Self { version: 1, hmac_key, accounts: BTreeMap::new() }
    }

    /// Creates a random stable DeviceId suitable for ActiveSync.
    pub fn device_id(length: usize) -> Result<String> {
        if !matches!(length, 16 | 32) {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                "profile Device ID length is unsupported",
            ));
        }
        let mut bytes = vec![0_u8; length / 2];
        rand::rng().fill_bytes(&mut bytes);
        Ok(bytes.iter().map(|byte| format!("{byte:02X}")).collect())
    }

    fn validate(&self) -> Result<()> {
        if self.version != 1 || self.hmac_key.len() != 32 {
            return Err(AppError::new(
                ErrorCode::StorageError,
                "credential-store secret bundle is invalid",
            ));
        }
        if self.accounts.values().any(|secret| {
            !matches!(secret.device_id.len(), 16 | 32)
                || !secret.device_id.bytes().all(|byte| byte.is_ascii_hexdigit())
                || (secret.policy_key == 0 && secret.policy.is_some())
        }) {
            return Err(AppError::new(
                ErrorCode::StorageError,
                "credential-store account secret is invalid",
            ));
        }
        Ok(())
    }
}

impl Default for SecretBundle {
    fn default() -> Self {
        Self::new()
    }
}

/// I/O boundary for the one versioned secret bundle.
pub trait SecretStore: Send + Sync {
    /// Loads the bundle, creating it when absent.
    fn load(&self) -> Result<SecretBundle>;
    /// Atomically updates the latest bundle across independent MCP processes.
    fn update(&self, action: &mut dyn FnMut(&mut SecretBundle) -> Result<()>) -> Result<()>;
    /// Removes the bundle.
    fn delete(&self) -> Result<()>;
}

/// Native macOS Keychain implementation.
#[derive(Debug, Clone)]
pub struct KeychainStore {
    journal_path: PathBuf,
}

impl KeychainStore {
    /// Uses the idempotency database as a cross-process Keychain update lock.
    #[must_use]
    pub fn new(journal_path: PathBuf) -> Self {
        Self { journal_path }
    }

    fn read_unlocked(&self) -> Result<SecretBundle> {
        let entry = keyring::Entry::new(SERVICE, BUNDLE_KEY).map_err(keychain_error)?;
        match entry.get_password() {
            Ok(value) => {
                let value = Zeroizing::new(value);
                let bundle: SecretBundle = serde_json::from_str(value.as_str()).map_err(|_| {
                    AppError::new(
                        ErrorCode::StorageError,
                        "credential-store secret bundle is invalid",
                    )
                })?;
                bundle.validate()?;
                Ok(bundle)
            }
            Err(keyring::Error::NoEntry) => {
                let bundle = SecretBundle::new();
                self.write_unlocked(&bundle)?;
                Ok(bundle)
            }
            Err(error) => Err(keychain_error(error)),
        }
    }

    fn write_unlocked(&self, bundle: &SecretBundle) -> Result<()> {
        bundle.validate()?;
        let entry = keyring::Entry::new(SERVICE, BUNDLE_KEY).map_err(keychain_error)?;
        let document = Zeroizing::new(serde_json::to_string(bundle).map_err(|_| {
            AppError::new(ErrorCode::StorageError, "cannot serialize credential-store bundle")
        })?);
        entry.set_password(document.as_str()).map_err(keychain_error)
    }
}

impl SecretStore for KeychainStore {
    fn load(&self) -> Result<SecretBundle> {
        with_storage_write_lock(&self.journal_path, || self.read_unlocked())
    }

    fn update(&self, action: &mut dyn FnMut(&mut SecretBundle) -> Result<()>) -> Result<()> {
        with_storage_write_lock(&self.journal_path, || {
            let mut bundle = self.read_unlocked()?;
            action(&mut bundle)?;
            self.write_unlocked(&bundle)
        })
    }

    fn delete(&self) -> Result<()> {
        with_storage_write_lock(&self.journal_path, || {
            let entry = keyring::Entry::new(SERVICE, BUNDLE_KEY).map_err(keychain_error)?;
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(keychain_error(error)),
            }
        })
    }
}

/// Deterministic in-memory secret store for tests and harness binaries.
#[derive(Default)]
pub struct MemorySecretStore {
    bundle: Mutex<Option<SecretBundle>>,
}

impl MemorySecretStore {
    /// Constructs a store containing a known bundle.
    #[must_use]
    pub fn with_bundle(bundle: SecretBundle) -> Self {
        Self { bundle: Mutex::new(Some(bundle)) }
    }
}

impl SecretStore for MemorySecretStore {
    fn load(&self) -> Result<SecretBundle> {
        let mut guard = self.bundle.lock().map_err(|_| lock_error())?;
        if guard.is_none() {
            *guard = Some(SecretBundle::new());
        }
        guard
            .clone()
            .ok_or_else(|| AppError::new(ErrorCode::StorageError, "secret bundle is unavailable"))
    }

    fn update(&self, action: &mut dyn FnMut(&mut SecretBundle) -> Result<()>) -> Result<()> {
        let mut guard = self.bundle.lock().map_err(|_| lock_error())?;
        let mut updated = guard.clone().unwrap_or_default();
        action(&mut updated)?;
        updated.validate()?;
        *guard = Some(updated);
        Ok(())
    }

    fn delete(&self) -> Result<()> {
        *self.bundle.lock().map_err(|_| lock_error())? = None;
        Ok(())
    }
}

fn keychain_error(_: keyring::Error) -> AppError {
    AppError::new(ErrorCode::AuthRequired, "operating-system credential store is unavailable")
        .remediation("Unlock the user credential store and retry")
}

fn lock_error() -> AppError {
    AppError::new(ErrorCode::StorageError, "secret store lock is unavailable")
}

#[cfg(test)]
mod tests;
