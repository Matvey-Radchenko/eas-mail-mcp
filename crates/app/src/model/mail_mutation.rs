use super::{MailDetail, OperationState};
use crate::ErrorEnvelope;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Follow-up flag status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MailFlagState {
    /// Remove the flag.
    None,
    /// Mark follow-up as active.
    Active,
    /// Mark follow-up as complete.
    Complete,
}
impl MailFlagState {
    pub(crate) const fn eas(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Active => 2,
            Self::Complete => 1,
        }
    }
}

/// Move a message into an existing mail folder in the same account.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailMoveInput {
    /// Portable reference to the message.
    pub mail_ref: String,
    /// Existing destination folder identifier in the same account.
    pub destination_folder_id: String,
    /// UUID retained for safe replay.
    pub idempotency_key: String,
}

/// Move a message into its account's system trash; never permanently deletes it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailDeleteInput {
    /// Portable reference to the message.
    pub mail_ref: String,
    /// UUID retained for safe replay.
    pub idempotency_key: String,
}

/// Change follow-up status while preserving supported flag metadata.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailSetFlagInput {
    /// Portable reference to the message.
    pub mail_ref: String,
    /// Desired flag state.
    pub flag: MailFlagState,
    /// UUID retained for safe replay.
    pub idempotency_key: String,
}

/// Replace the message's category set.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailSetCategoriesInput {
    /// Portable reference to the message.
    pub mail_ref: String,
    /// At most 50 distinct names, each 1–255 characters; empty clears categories.
    pub categories: Vec<String>,
    /// UUID retained for safe replay.
    pub idempotency_key: String,
}

/// One independently journaled action permitted in a batch.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum MailAction {
    /// Replace read state.
    MarkRead {
        /// Desired read state.
        is_read: bool,
    },
    /// Move into an existing folder in the same account.
    Move {
        /// Destination folder identifier.
        destination_folder_id: String,
    },
    /// Move to system trash, without permanent deletion.
    Delete,
    /// Replace flag status.
    SetFlag {
        /// Desired flag state.
        flag: MailFlagState,
    },
    /// Replace the category set.
    SetCategories {
        /// New set; empty clears it.
        categories: Vec<String>,
    },
}

/// A single batch entry with its own UUID.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MailBatchItem {
    /// Portable message reference.
    pub mail_ref: String,
    /// UUID for this entry only.
    pub idempotency_key: String,
    /// Requested operation.
    #[serde(flatten)]
    pub action: MailAction,
}
impl MailBatchItem {
    pub(crate) const fn kind(&self) -> &'static str {
        match self.action {
            MailAction::MarkRead { .. } => "mail_mark_read",
            MailAction::Move { .. } => "mail_move",
            MailAction::Delete => "mail_delete",
            MailAction::SetFlag { .. } => "mail_set_flag",
            MailAction::SetCategories { .. } => "mail_set_categories",
        }
    }
}
/// Apply up to 20 independently journaled property changes, moves, or trash operations.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailBatchInput {
    /// Unique messages and unique UUIDs. Unknown outcomes stop remaining entries for that account.
    pub items: Vec<MailBatchItem>,
}

/// Confirmed result of one message mutation.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MailMutationResult {
    /// Durable operation UUID.
    pub operation_id: String,
    /// Confirmed state or retained replay state.
    pub status: OperationState,
    /// Safe operation summary.
    pub message: String,
    /// Resulting portable reference, including the new locator after a move.
    pub mail_ref: Option<String>,
}

/// Batch entry result in original input order.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MailBatchEntry {
    /// Original input reference.
    pub mail_ref: String,
    /// Entry's UUID.
    pub operation_id: String,
    /// Successful or historical result.
    pub result: Option<MailMutationResult>,
    /// Entry failure or skipped-after-unknown reason.
    pub error: Option<ErrorEnvelope>,
    /// Whether execution was stopped before this entry began.
    pub skipped: bool,
}

/// Complete bounded batch report; per-entry errors remain available.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MailBatchData {
    /// Results in input order.
    pub items: Vec<MailBatchEntry>,
}

/// Fetch up to 20 unique messages with a shared text budget.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailGetManyInput {
    /// Unique portable references.
    pub mail_refs: Vec<String>,
    /// Per-message characters, default 12,000 and maximum 50,000.
    pub body_limit: Option<u32>,
    /// Shared body character budget, default and maximum 100,000.
    pub total_body_limit: Option<u32>,
}

/// One bulk read result, retaining a safe per-item error.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MailGetManyEntry {
    /// Requested reference.
    pub mail_ref: String,
    /// Message when available.
    pub mail: Option<MailDetail>,
    /// Safe failure when unavailable.
    pub error: Option<ErrorEnvelope>,
}

/// Bulk read report with an explicit truncation marker.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MailGetManyData {
    /// Results in request order.
    pub items: Vec<MailGetManyEntry>,
    /// Whether one or more bodies were limited by the server or shared budget.
    pub bodies_truncated: bool,
}
