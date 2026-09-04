use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::OperationStatus;

/// Read-only lookup of one durable operation UUID.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OperationGetInput {
    /// UUID returned by a mutation or supplied as its idempotency key.
    pub operation_id: String,
}

/// Bounded newest-first lookup of durable operation metadata.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OperationsListInput {
    /// Optional exact local account identifier.
    pub account_id: Option<String>,
    /// Optional durable state filter.
    pub status: Option<OperationStatus>,
    /// Maximum rows, default 20 and maximum 100.
    pub limit: Option<u16>,
}

/// Content-free state usable to investigate uncertain external writes.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct OperationMetadata {
    /// Durable operation UUID.
    pub operation_id: String,
    /// Local owning account identifier.
    pub account_id: String,
    /// Stable mutation tool name.
    pub kind: String,
    /// Durable state; pending has no final checkpoint and may be active or interrupted.
    pub status: OperationStatus,
    /// Original creation time.
    pub created_at: DateTime<Utc>,
    /// Last durable change time.
    pub updated_at: DateTime<Utc>,
    /// Bit mask of durably confirmed steps; zero does not prove no external action occurred.
    pub completed_steps: u32,
}

/// Bounded durable operation list.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct OperationsData {
    /// Newest operations first, with no mailbox fields or payload hashes.
    pub operations: Vec<OperationMetadata>,
}
