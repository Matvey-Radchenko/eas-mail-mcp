use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Opt-in persistent calendar synchronization; no SyncKey is required.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarSyncInput {
    /// One configured account.
    pub account_id: String,
    /// Optional calendar collection; omission synchronizes every calendar.
    pub collection_id: Option<String>,
    /// Optional expected saved key. Requires a matching saved snapshot; never logged.
    pub sync_key: Option<String>,
    /// Rebuild the selected collections while retaining the last complete snapshots.
    #[serde(default)]
    pub full: bool,
    /// First local date, default today minus 90 days.
    pub date_from: Option<String>,
    /// Last local date, default today plus 90 days.
    pub date_to: Option<String>,
    /// IANA timezone, default UTC.
    pub time_zone: Option<String>,
    /// Maximum Sync pages per collection per invocation, default 20, maximum 100.
    pub max_pages: Option<u16>,
}

/// One complete local occurrence, including participants supplied by Calendar Sync.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarSyncItem {
    /// Whether a complete body was included; false must not erase a client's cached notes.
    pub body_available: bool,
    /// Sanitized event details, without a separate ItemOperations read.
    #[serde(flatten)]
    pub event: super::CalendarEvent,
    /// Whether this is an expanded recurrence instance.
    pub recurring: bool,
    /// Reminder minutes; null clears the reminder.
    pub reminder: Option<u32>,
}

/// Redacted collection progress. SyncKeys are kept private in the backend.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarSyncCollection {
    /// Exchange collection identifier.
    pub collection_id: String,
    /// A complete baseline is available, possibly from before a rebuild.
    pub ready: bool,
    /// More pages must be requested to finish the current pass.
    pub more_available: bool,
    /// Count of retained master events in the available snapshot.
    pub event_count: usize,
    /// Policy-enforced server filter, distinct from local display dates.
    pub filter_type: u8,
}

/// A local projection after applying incremental server changes.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarSyncData {
    /// Owning account.
    pub account_id: String,
    /// All selected collections have a complete available baseline.
    pub ready: bool,
    /// This pass drained all selected collections.
    pub complete: bool,
    /// Sync pages requested, excluding discovery/provisioning.
    pub pages: usize,
    /// Server Add/Change/Delete/SoftDelete commands received in this invocation.
    pub changes_applied: usize,
    /// Complete locally expanded display window; not a network re-download.
    pub items: Vec<CalendarSyncItem>,
    /// Per-collection progress.
    pub collections: Vec<CalendarSyncCollection>,
}
