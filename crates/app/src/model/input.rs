use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Maximum number of Unicode scalar values accepted in an outgoing body.
pub const MAX_OUTGOING_BODY_CHARS: usize = 50_000;

/// Optional account selection shared by read tools.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountSelection {
    /// Account IDs; omitted means all enabled accounts.
    pub account_ids: Option<Vec<String>>,
}

/// Scope accepted by `sync_now`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SyncScope {
    /// Synchronize mail only.
    Mail,
    /// Synchronize calendars only.
    Calendar,
    /// Synchronize mail and calendars.
    #[default]
    All,
}

/// Input for explicit synchronization.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SyncInput {
    /// Account IDs; omitted means all enabled accounts.
    pub account_ids: Option<Vec<String>>,
    /// Collection scope.
    #[serde(default)]
    pub scope: SyncScope,
}

/// Input for paginated mail listing.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailListInput {
    /// Account IDs; omitted means all enabled accounts.
    pub account_ids: Option<Vec<String>>,
    /// Optional Exchange folder IDs; omitted defaults to Inbox and Sent.
    pub folder_ids: Option<Vec<String>>,
    /// Opaque 15-minute snapshot cursor.
    pub cursor: Option<String>,
    /// Number of items, from 1 through 100.
    pub limit: Option<u8>,
}

/// Input for server-side mailbox search.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailSearchInput {
    /// Search text sent to EAS Search.
    pub query: String,
    /// Account IDs; omitted means all enabled accounts.
    pub account_ids: Option<Vec<String>>,
    /// Opaque 15-minute snapshot cursor.
    pub cursor: Option<String>,
    /// Number of items, from 1 through 100.
    pub limit: Option<u8>,
}

/// Input for a full mail fetch.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailGetInput {
    /// Process-local mail reference from list or search.
    pub mail_ref: String,
    /// Requested body characters: default 12,000, maximum 50,000.
    pub body_limit: Option<u32>,
}

/// Input for mail attachment metadata.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailAttachmentsInput {
    /// Process-local mail reference.
    pub mail_ref: String,
}

/// Input for attachment download.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachmentDownloadInput {
    /// Process-local attachment reference.
    pub attachment_ref: String,
}

/// Input for paginated calendar listing.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarListInput {
    /// Account IDs; omitted means all enabled accounts.
    pub account_ids: Option<Vec<String>>,
    /// Optional calendar folder IDs.
    pub folder_ids: Option<Vec<String>>,
    /// Opaque 15-minute snapshot cursor.
    pub cursor: Option<String>,
    /// Number of items, from 1 through 100.
    pub limit: Option<u8>,
}

/// Input for refreshed in-memory calendar search.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarSearchInput {
    /// Case-insensitive text matched against safe event fields.
    pub query: String,
    /// Account IDs; omitted means all enabled accounts.
    pub account_ids: Option<Vec<String>>,
    /// Opaque 15-minute snapshot cursor.
    pub cursor: Option<String>,
    /// Number of items, from 1 through 100.
    pub limit: Option<u8>,
}

/// Input for one calendar event.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarGetInput {
    /// Process-local calendar event reference.
    pub event_ref: String,
}

/// Input for changing read state.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkReadInput {
    /// Process-local mail reference.
    pub mail_ref: String,
    /// New read state.
    pub is_read: bool,
    /// UUID used for operation idempotency.
    pub idempotency_key: String,
}

/// Input for sending a plain-text message.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailSendInput {
    /// Sending account ID.
    pub account_id: String,
    /// To recipients.
    pub to: Vec<String>,
    /// Cc recipients.
    #[serde(default)]
    pub cc: Vec<String>,
    /// Bcc recipients.
    #[serde(default)]
    pub bcc: Vec<String>,
    /// Subject, maximum 998 characters.
    pub subject: String,
    /// Plain-text body, maximum 50,000 Unicode scalar values.
    #[schemars(length(max = 50_000))]
    pub body: String,
    /// UUID used for operation idempotency.
    pub idempotency_key: String,
}

/// Input for SmartReply.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailReplyInput {
    /// Process-local source mail reference.
    pub mail_ref: String,
    /// Plain-text reply body, maximum 50,000 Unicode scalar values.
    #[schemars(length(max = 50_000))]
    pub body: String,
    /// Include original To and Cc recipients.
    #[serde(default)]
    pub reply_all: bool,
    /// UUID used for operation idempotency.
    pub idempotency_key: String,
}

/// Input for SmartForward.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailForwardInput {
    /// Process-local source mail reference.
    pub mail_ref: String,
    /// Forward recipients.
    pub to: Vec<String>,
    /// Cc recipients.
    #[serde(default)]
    pub cc: Vec<String>,
    /// Bcc recipients.
    #[serde(default)]
    pub bcc: Vec<String>,
    /// Optional plain-text introduction, maximum 50,000 Unicode scalar values.
    #[serde(default)]
    #[schemars(length(max = 50_000))]
    pub body: String,
    /// UUID used for operation idempotency.
    pub idempotency_key: String,
}
