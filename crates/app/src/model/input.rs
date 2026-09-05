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
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailSearchInput {
    /// Search text sent to EAS Search; omit only with both date bounds spanning at most 31 days.
    #[serde(default)]
    pub query: String,
    /// Exact filters over a bounded server candidate set; inspect response coverage.
    #[serde(flatten)]
    pub filters: super::MailSearchFilters,
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

/// A local file to attach to an outgoing message. Bytes are read only for this operation.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutgoingAttachmentInput {
    /// Absolute path to a regular local file; links and reparse points are rejected.
    pub path: String,
    /// Optional display filename, at most 255 UTF-8 bytes; defaults to the path's filename.
    pub filename: Option<String>,
    /// Optional MIME type without parameters; defaults to application/octet-stream.
    pub content_type: Option<String>,
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
    /// Local attachments: at most 20 files and 25 MiB of raw bytes in total.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 20))]
    pub attachments: Vec<OutgoingAttachmentInput>,
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
    /// Local attachments: at most 20 files and 25 MiB of raw bytes in total.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 20))]
    pub attachments: Vec<OutgoingAttachmentInput>,
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
    /// Local attachments: at most 20 files and 25 MiB of raw bytes in total.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 20))]
    pub attachments: Vec<OutgoingAttachmentInput>,
    /// UUID used for operation idempotency.
    pub idempotency_key: String,
}
