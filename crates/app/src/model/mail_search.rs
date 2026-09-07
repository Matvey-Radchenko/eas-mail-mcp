use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::MailDetail;

/// Optional exact metadata filters, applied to bounded EAS Search candidates.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct MailSearchFilters {
    /// Exact SMTP sender address, matched case-insensitively.
    pub from: Option<String>,
    /// Exact SMTP address present in the To header, matched case-insensitively.
    pub to: Option<String>,
    /// Exclusive receive-time lower bound, RFC3339 with an explicit offset.
    pub received_after: Option<DateTime<Utc>>,
    /// Exclusive receive-time upper bound, RFC3339 with an explicit offset.
    pub received_before: Option<DateTime<Utc>>,
    /// Exact read state; missing metadata never counts as false.
    pub is_read: Option<bool>,
    /// Exact attachment presence; missing metadata never counts as false.
    pub has_attachments: Option<bool>,
    /// Explicit mail folder IDs; subfolders are not implicitly included.
    #[serde(default)]
    pub folder_ids: Vec<String>,
}

/// Coverage of one account's bounded search candidate set.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct MailSearchCoverage {
    /// Account whose search produced these candidates.
    pub account_id: String,
    /// Unique candidates examined, never more than 1000.
    pub candidates_examined: usize,
    /// EAS Search calls issued, never more than 10.
    pub search_calls: usize,
    /// Optional estimated server count before local metadata filtering.
    pub estimated_total: Option<usize>,
    /// Whether the candidate set was exhausted within the budget.
    pub candidates_complete: bool,
    /// Candidates that could not be evaluated because required metadata was absent.
    pub metadata_unknown: usize,
}

/// Reads one server conversation by a portable message reference.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MailGetThreadInput {
    /// Portable mail reference from list, search, or an earlier process.
    pub mail_ref: String,
    /// Maximum messages to read, default 20 and maximum 100.
    #[schemars(range(min = 1, max = 100))]
    pub limit: Option<u8>,
    /// Per-message body characters, default 12000 and maximum 50000.
    #[schemars(range(min = 1, max = 50_000))]
    pub body_limit: Option<u32>,
    /// Total body characters, default and maximum 100000.
    #[schemars(range(min = 1, max = 100_000))]
    pub total_body_limit: Option<u32>,
}

/// Bounded chronological messages from one Exchange conversation.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MailThreadData {
    /// Chronologically ordered messages; every body has its own truncation flag.
    pub items: Vec<MailDetail>,
    /// Whether some messages or candidates were not returned.
    pub results_truncated: bool,
    /// Whether any returned body was truncated by a server or application budget.
    pub bodies_truncated: bool,
    /// Search scope and budget actually examined.
    pub coverage: MailSearchCoverage,
}
