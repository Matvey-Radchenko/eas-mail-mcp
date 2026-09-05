use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{CalendarFindSlotsInput, CalendarSlotParticipant, ScheduleWeekday, WorkingHoursInput};

/// Whether a participant must be available for a one-off suggestion.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CalendarParticipantRole {
    /// Exclude one-off suggestions with any conflict for this participant.
    #[default]
    Required,
    /// Keep suggestions and report conflicts for this participant.
    Optional,
}

/// Scheduling preferences for one entry in `participants`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarParticipantOptions {
    /// Exact participant input, ignoring leading and trailing whitespace.
    pub input: String,
    /// Defaults to required; at least one participant must remain required.
    #[serde(default)]
    pub role: CalendarParticipantRole,
    /// IANA timezone; omitted inherits the request timezone.
    pub time_zone: Option<String>,
    /// Local working hours; omitted inherits request wall-clock hours in this person's timezone.
    pub working_hours: Option<Vec<WorkingHoursInput>>,
}

/// Why a participant cannot safely attend one proposed occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CalendarSlotConflictReason {
    /// The meeting extends outside this participant's local working hours.
    OutsideWorkingHours,
    /// Exchange reports busy time, including the requested buffer.
    Busy,
    /// Exchange reports out-of-office time, including the requested buffer.
    OutOfOffice,
    /// Tentative time is blocked unless explicitly allowed.
    Tentative,
    /// Missing data, unavailable intervals, or unresolved directory identity.
    Unknown,
}

/// One participant's conflict; multiple reasons may apply.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarSlotConflict {
    /// Original participant input.
    pub input: String,
    /// Required or optional participant.
    pub role: CalendarParticipantRole,
    /// Explicit reasons; unknown data is never interpreted as free.
    pub reasons: Vec<CalendarSlotConflictReason>,
}

/// One concrete candidate, ranked with required participants kept conflict-free.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarSlotSuggestion {
    /// Meeting start in the requested timezone.
    pub starts_at: String,
    /// Meeting end in the requested timezone.
    pub ends_at: String,
    /// Participants with conflicts or unknown data.
    pub conflicts: Vec<CalendarSlotConflict>,
    /// Participants whose tentative time was accepted by `allow_tentative`.
    pub tentative_participants: Vec<String>,
}

/// Weekly recurring search over at most 90 inclusive days and 13 occurrences.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarFindRecurringSlotsInput {
    /// Scheduling constraints. `limit` defaults to five patterns, maximum ten.
    #[serde(flatten)]
    pub schedule: CalendarFindSlotsInput,
    /// One weekday repeated at the same local wall-clock time in the request timezone.
    pub weekday: ScheduleWeekday,
}

/// One weekly pattern; every occurrence carries its own conflicts.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarRecurringSlotSuggestion {
    /// Weekly local start time, in HH:MM format.
    pub local_start_time: String,
    /// Number of occurrences without any required-participant conflict.
    pub required_available_occurrences: u8,
    /// Explicit dates, participants and conflict reasons for every occurrence.
    pub occurrences: Vec<CalendarSlotSuggestion>,
}

/// Bounded ranked weekly patterns; required-participant conflicts are allowed and explicit.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarRecurringSlotsData {
    /// Account used for Exchange availability.
    pub account_id: String,
    /// Timezone preserving the weekly wall-clock time across DST.
    pub time_zone: String,
    /// Repeated weekday.
    pub weekday: ScheduleWeekday,
    /// Requested meeting duration.
    pub duration_minutes: u16,
    /// Requested minimum break before and after the meeting.
    pub buffer_minutes: u16,
    /// Server free/busy precision remains 30 minutes.
    pub precision_minutes: u8,
    /// Whether every participant resolved exactly.
    pub resolution_complete: bool,
    /// Participant identity and availability summaries.
    pub participants: Vec<CalendarSlotParticipant>,
    /// Ranked patterns, best required-participant coverage first.
    pub suggestions: Vec<CalendarRecurringSlotSuggestion>,
    /// Whether additional valid patterns were omitted by the result limit.
    pub results_truncated: bool,
}
