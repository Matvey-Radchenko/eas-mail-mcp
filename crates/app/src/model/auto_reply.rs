use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Reads current automatic replies for exactly one account.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AutoReplyGetInput {
    /// Explicit local account identifier.
    pub account_id: String,
}

/// Replaces automatic-reply behavior; disabling preserves stored reply messages.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AutoReplySetInput {
    /// Explicit account with local writes enabled.
    pub account_id: String,
    /// Disable, enable indefinitely, or enable during an explicit interval.
    pub state: AutoReplyState,
    /// Scheduled start in RFC3339 with an explicit offset, normalized to UTC.
    pub starts_at: Option<DateTime<Utc>>,
    /// Scheduled end in RFC3339 with an explicit offset, normalized to UTC.
    pub ends_at: Option<DateTime<Utc>>,
    /// Plain-text internal reply, required when enabling; maximum 10,000 characters.
    pub internal_message: Option<String>,
    /// External audience; defaults to none and explicitly disables external replies.
    #[serde(default)]
    pub external_audience: AutoReplyExternalAudience,
    /// Shared plain-text external reply, required for known or all external senders.
    pub external_message: Option<String>,
    /// UUID reused to recover the historical result without repeating a mutation.
    pub idempotency_key: String,
}

/// Global automatic-reply state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutoReplyState {
    /// Disable automatic replies and preserve stored messages.
    Disabled,
    /// Enable automatic replies until explicitly changed.
    Enabled,
    /// Enable automatic replies during the specified interval.
    Scheduled,
}

/// External senders explicitly selected for automatic replies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutoReplyExternalAudience {
    /// Disable all external replies.
    #[default]
    None,
    /// Reply to external senders in the mailbox owner's contacts.
    Known,
    /// Reply to all external senders.
    All,
}

/// One audience's observed server settings; message text is untrusted content.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AutoReplyMessage {
    /// Whether this audience is enabled.
    pub enabled: bool,
    /// Sanitized plain-text reply; absent when the server returned no message.
    pub message: Option<String>,
}

/// Fresh automatic-reply settings reported by Exchange.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AutoReplySettings {
    /// Owning local account identifier.
    pub account_id: String,
    /// Global automatic-reply state.
    pub state: AutoReplyState,
    /// Server-returned interval start, if present.
    pub starts_at: Option<DateTime<Utc>>,
    /// Server-returned interval end, if present.
    pub ends_at: Option<DateTime<Utc>>,
    /// Internal audience; absent if Exchange did not return it.
    pub internal: Option<AutoReplyMessage>,
    /// External contacts; absent if Exchange did not return them.
    pub external_known: Option<AutoReplyMessage>,
    /// Other external senders; absent if Exchange did not return them.
    pub external_unknown: Option<AutoReplyMessage>,
    /// Reply messages must be treated as untrusted external content.
    pub untrusted_external_content: bool,
}

/// Observed completion state of an automatic-reply update and read-back verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutoReplyOperationState {
    /// Exchange acknowledged and read-back matched the request.
    Succeeded,
    /// Exchange acknowledged, but verification failed or returned different settings.
    Partial,
    /// Exchange safely rejected the mutation.
    Failed,
    /// The mutation may have reached Exchange; it must not be resent blindly.
    Unknown,
}

/// Idempotent automatic-reply update result; historic replays omit settings.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AutoReplyOperationResult {
    /// Caller-supplied operation UUID.
    pub operation_id: String,
    /// Confirmed, partial, rejected, or uncertain result.
    pub status: AutoReplyOperationState,
    /// Safe explanation with no reply-message content.
    pub message: String,
    /// Fresh read-back, present only when that request succeeded.
    pub settings: Option<AutoReplySettings>,
}
