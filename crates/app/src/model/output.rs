use crate::ErrorEnvelope;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Account-scoped warning returned alongside partial results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Warning {
    /// Affected account; empty for a historic operation warning identified by its UUID.
    pub account_id: String,
    /// Stable safe code.
    pub code: String,
    /// Safe warning text.
    pub message: String,
    /// Whether a later retry of the failed read can be useful.
    #[serde(default)]
    pub retryable: bool,
    /// Safe recovery instructions for this account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
    /// Operation UUID associated with this warning, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// Server-advertised delay for a safe retry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
}

/// Uniform structured MCP response.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ApiResponse<T> {
    /// Successful typed payload.
    pub data: Option<T>,
    /// Fatal error when no result can be returned.
    pub error: Option<ErrorEnvelope>,
    /// Non-fatal per-account failures.
    pub warnings: Vec<Warning>,
}

impl<T> ApiResponse<T> {
    /// Constructs a successful response.
    #[must_use]
    pub fn success(data: T, warnings: Vec<Warning>) -> Self {
        Self { data: Some(data), error: None, warnings }
    }

    /// Constructs a failed response.
    #[must_use]
    pub fn failure(error: ErrorEnvelope) -> Self {
        Self { data: None, error: Some(error), warnings: Vec::new() }
    }
}

/// One configured account visible to agents.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AccountStatus {
    /// Stable local account ID.
    pub account_id: String,
    /// Fixed managed profile.
    pub profile: String,
    /// Mailbox address.
    pub email: String,
    /// Whether this account is enabled.
    pub enabled: bool,
    /// Current process-local status.
    pub status: String,
}

/// Accounts response payload.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AccountsData {
    /// Configured accounts.
    pub accounts: Vec<AccountStatus>,
}

/// One Exchange folder.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FolderView {
    /// Owning account ID.
    pub account_id: String,
    /// Exchange folder ID.
    pub folder_id: String,
    /// Display name supplied by Exchange.
    pub display_name: String,
    /// `mail`, `calendar`, or `other`.
    pub kind: String,
    /// Stable EAS folder role such as `inbox`, `sent`, or `user_mail`.
    pub role: String,
    /// External content marker.
    pub untrusted_external_content: bool,
}

/// Folders response payload.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FoldersData {
    /// Exchange folders.
    pub folders: Vec<FolderView>,
}

/// Process synchronization report.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SyncReport {
    /// Account ID.
    pub account_id: String,
    /// Synchronization scope, currently always `mail`.
    pub scope: String,
    /// Collections synchronized.
    pub collections_synced: usize,
    /// Changes applied to RAM.
    pub changes_applied: usize,
    /// Completion time.
    pub completed_at: DateTime<Utc>,
}

/// Synchronization response payload.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SyncData {
    /// Per-account reports.
    pub reports: Vec<SyncReport>,
}

/// Safe message summary.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MailSummary {
    /// Portable opaque mail reference.
    pub mail_ref: String,
    /// Owning account ID.
    pub account_id: String,
    /// Exchange folder ID.
    pub folder_id: String,
    /// Subject.
    pub subject: String,
    /// Sender.
    pub sender: String,
    /// Recipients.
    pub recipients: String,
    /// Receive time.
    pub received_at: Option<DateTime<Utc>>,
    /// Plain preview, maximum 500 characters.
    pub preview: String,
    /// Read state.
    pub is_read: bool,
    /// Whether attachments are present.
    pub has_attachments: bool,
    /// Flag state when Exchange supplied recognized metadata; null means unknown.
    pub flag: Option<super::MailFlagState>,
    /// Category set when Exchange supplied metadata; null means unknown.
    pub categories: Option<Vec<String>>,
    /// Meeting request, update, cancellation, response, or other Calendar mail classification.
    pub calendar_message: Option<CalendarMailKind>,
    /// Whether this mail reference can be passed to `calendar_respond`.
    pub can_respond: bool,
    /// External content marker.
    pub untrusted_external_content: bool,
}

/// Calendar semantics attached to an Exchange mail item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CalendarMailKind {
    /// Initial actionable meeting request.
    Request,
    /// Meeting update; full updates can be actionable.
    Update,
    /// Meeting cancellation.
    Cancellation,
    /// Attendee response sent to an organizer.
    Response,
    /// Unrecognized Calendar message subtype.
    Other,
}

/// Paginated mail response payload.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MailPage {
    /// Message summaries.
    pub items: Vec<MailSummary>,
    /// Cursor for the same immutable snapshot.
    pub next_cursor: Option<String>,
    /// Whether the search candidate set or required metadata was incomplete.
    pub results_truncated: bool,
    /// Per-account search coverage; empty for collection listing.
    pub coverage: Vec<super::MailSearchCoverage>,
}

/// On-demand full message body.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MailDetail {
    /// Summary fields.
    #[serde(flatten)]
    pub summary: MailSummary,
    /// Cc recipients.
    pub cc: String,
    /// Sanitized plain-text body.
    pub body: String,
    /// Whether Exchange or the application truncated the body.
    pub body_truncated: bool,
}

/// Attachment metadata with portable opaque references.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AttachmentView {
    /// Portable opaque attachment reference.
    pub attachment_ref: String,
    /// Owning account ID.
    pub account_id: String,
    /// Safe display name.
    pub display_name: String,
    /// Estimated size.
    pub size: u64,
    /// MIME type.
    pub content_type: String,
    /// Inline marker.
    pub is_inline: bool,
    /// External content marker.
    pub untrusted_external_content: bool,
}

/// Attachment metadata payload.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AttachmentsData {
    /// Attachments on a message.
    pub attachments: Vec<AttachmentView>,
}

/// Managed temporary attachment file.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AttachmentDownload {
    /// Private temporary path.
    pub path: String,
    /// Expiry time; the process may delete the file sooner on shutdown.
    pub expires_at: DateTime<Utc>,
}

/// Stable operation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    /// Mutation completed successfully.
    Succeeded,
    /// Mutation was rejected before an ambiguous send.
    Failed,
    /// Mutation may have reached Exchange.
    Unknown,
}

/// Idempotent mutation response.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct OperationResult {
    /// UUID supplied by the caller.
    pub operation_id: String,
    /// Final or unknown state.
    pub status: OperationState,
    /// Safe status text.
    pub message: String,
}
