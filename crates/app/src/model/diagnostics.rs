use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ErrorCode;

/// Fresh connection checks for individually selected accounts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AccountsStatusData {
    /// Per-account results, including failures when every account is unavailable.
    pub accounts: Vec<AccountHealth>,
}

/// Configuration and the result of a fresh read-only EAS probe.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AccountHealth {
    /// Local account identifier used by other tools.
    pub account_id: String,
    /// Whether this account is enabled in local configuration.
    pub enabled: bool,
    /// Whether local configuration permits mutations.
    pub write_enabled: bool,
    /// Effective server-side write permission; null because diagnostics never issue test writes.
    pub server_write_permission: Option<bool>,
    /// Current connection state; disabled accounts are not probed.
    pub status: AccountHealthStatus,
    /// Stable error category when the probe failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<ErrorCode>,
    /// Advertised features, present only after a successful probe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<AccountCapabilities>,
}

/// Result of an account health check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccountHealthStatus {
    /// Configuration is enabled and the server probe succeeded.
    Ready,
    /// Local configuration disables this account.
    Disabled,
    /// The server probe or local credential boundary failed.
    Failed,
}

/// Explicitly advertised server features; local write permission is separate.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
pub struct AccountCapabilities {
    /// ResolveRecipients availability is advertised.
    pub calendar_availability: bool,
    /// Mail compose commands are advertised.
    pub mail_writes: bool,
    /// Calendar Add, Change, and Delete are available.
    pub personal_calendar_writes: bool,
    /// Meeting notifications and responses are available.
    pub meeting_lifecycle: bool,
    /// Settings is advertised; effective permission has not been tested by a write.
    pub auto_reply: bool,
    /// MoveItems is advertised; effective permission has not been tested by a write.
    pub mail_move: bool,
    /// Sync is advertised; effective permission has not been tested by a write.
    pub mail_properties: bool,
}

impl From<crate::backend::BackendCapabilities> for AccountCapabilities {
    fn from(value: crate::backend::BackendCapabilities) -> Self {
        Self {
            calendar_availability: value.calendar_availability,
            mail_writes: value.mail_writes,
            personal_calendar_writes: value.personal_calendar_writes,
            meeting_lifecycle: value.meeting_lifecycle,
            auto_reply: value.auto_reply,
            mail_move: value.mail_move,
            mail_properties: value.mail_properties,
        }
    }
}

/// Aggregate local attachment-cache usage without file names or paths.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CacheStatusData {
    /// Regular files currently stored, including expired files awaiting cleanup.
    pub files: u64,
    /// Total regular-file size in bytes.
    pub bytes: u64,
    /// Files eligible for expiry cleanup.
    pub expired_files: u64,
    /// Size of expired regular files in bytes.
    pub expired_bytes: u64,
    /// Number of hours before a downloaded file becomes eligible for cleanup.
    pub retention_hours: u32,
    /// Cleanup trigger description; there is no background deletion timer.
    pub cleanup_policy: String,
}

/// Aggregate result of explicitly clearing downloaded attachments.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CacheClearData {
    /// Number of removed regular files.
    pub removed_files: u64,
    /// Size of removed regular files in bytes.
    pub removed_bytes: u64,
    /// Number of regular files remaining across the cache.
    pub remaining_files: u64,
    /// Size of remaining regular files in bytes.
    pub remaining_bytes: u64,
}
