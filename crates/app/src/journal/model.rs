use serde::{Deserialize, Serialize};

use crate::{AppError, ErrorCode, Result};

/// Durable operation state stored without mailbox content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    /// Written before the Exchange request; its owner may still be running.
    Pending,
    /// Exchange confirmed success.
    Succeeded,
    /// Exchange safely rejected the request.
    Failed,
    /// Some confirmed steps succeeded before a later safe failure.
    Partial,
    /// The request may have reached Exchange.
    Unknown,
}

impl OperationStatus {
    /// Returns the stable journal and API spelling of this state.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Partial => "partial",
            Self::Unknown => "unknown",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "partial" => Ok(Self::Partial),
            "unknown" => Ok(Self::Unknown),
            _ => Err(super::storage_error()),
        }
    }
}

/// Content-free operation journal row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalRecord {
    /// Caller UUID.
    pub operation_id: String,
    /// Account ID.
    pub account_id: String,
    /// Mutation kind.
    pub kind: String,
    /// HMAC of the canonical payload.
    pub payload_hmac: String,
    /// Stable EAS ClientId.
    pub client_id: String,
    /// Durable state.
    pub status: OperationStatus,
    /// Content-free bit mask of confirmed lifecycle steps.
    pub completed_steps: u32,
}

/// Whether `begin` inserted a row or found the same prior operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalBegin {
    /// Durable row.
    pub record: JournalRecord,
    /// True only for the caller that inserted the pending row.
    pub inserted: bool,
}

/// Destination locator returned by a confirmed move, without mailbox content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailResultLocator {
    /// Destination collection identifier within the operation's account.
    pub folder_id: String,
    /// New opaque item identifier within that collection.
    pub server_id: String,
}

impl MailResultLocator {
    /// Validates the same bounded locator fields used by portable references.
    pub fn validate(&self) -> Result<()> {
        if [&self.folder_id, &self.server_id].into_iter().any(|value| {
            value.is_empty() || value.len() > 8192 || value.chars().any(char::is_control)
        }) {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                "operation result locator is invalid or exceeds 8 KiB per identifier",
            ));
        }
        Ok(())
    }
}

/// Inspectable durable metadata for one operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEntry {
    /// Idempotency state and confirmed steps.
    pub record: JournalRecord,
    /// Creation time as Unix seconds.
    pub created_at: i64,
    /// Last durable state change as Unix seconds.
    pub updated_at: i64,
    /// Confirmed destination locator, when available.
    pub result_locator: Option<MailResultLocator>,
}

/// Bounded filters for newest-first operation inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalFilter {
    /// Optional exact account identifier.
    pub account_id: Option<String>,
    /// Optional exact operation state.
    pub status: Option<OperationStatus>,
    /// Maximum entries, from 1 through 100.
    pub limit: u16,
}

impl Default for JournalFilter {
    fn default() -> Self {
        Self { account_id: None, status: None, limit: 20 }
    }
}

impl JournalFilter {
    /// Rejects unbounded inspection and invalid account identifiers.
    pub fn validate(&self) -> Result<()> {
        if !(1..=100).contains(&self.limit)
            || self.account_id.as_ref().is_some_and(|id| !crate::config::valid_account_id(id))
        {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                "operation filters require a valid account and a limit between 1 and 100",
            ));
        }
        Ok(())
    }
}
