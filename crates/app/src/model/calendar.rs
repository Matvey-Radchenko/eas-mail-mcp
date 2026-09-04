use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{CalendarAttendeeStatus, CalendarAttendeeView, CalendarBusyStatus, CalendarEventType};

/// Weekday used by explicit scheduling windows.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleWeekday {
    /// Monday.
    Mon,
    /// Tuesday.
    Tue,
    /// Wednesday.
    Wed,
    /// Thursday.
    Thu,
    /// Friday.
    Fri,
    /// Saturday.
    Sat,
    /// Sunday.
    Sun,
}

/// One explicit local-time working interval applied to selected weekdays.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkingHoursInput {
    /// Weekdays receiving this interval.
    #[schemars(length(min = 1, max = 7))]
    pub weekdays: Vec<ScheduleWeekday>,
    /// Local start time in `HH:MM` format.
    pub start: String,
    /// Local end time in `HH:MM` format; overnight intervals are unsupported.
    pub end: String,
}

/// Input for compact free/busy schedules.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarAvailabilityInput {
    /// Account used to query Exchange; inferred only when unambiguous.
    pub account_id: Option<String>,
    /// Directory names or email addresses, from 1 through 20.
    #[schemars(length(min = 1, max = 20))]
    pub participants: Vec<String>,
    /// First local date in `YYYY-MM-DD` format.
    pub date_from: String,
    /// Last inclusive local date in `YYYY-MM-DD` format.
    pub date_to: String,
    /// IANA timezone such as `Europe/Belgrade`.
    pub time_zone: String,
    /// Explicit local-time windows to include.
    #[schemars(length(min = 1))]
    pub working_hours: Vec<WorkingHoursInput>,
}

/// Input for common free-window calculation.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarFindSlotsInput {
    /// Account used to query Exchange; inferred only when unambiguous.
    pub account_id: Option<String>,
    /// Directory names or email addresses, from 1 through 20.
    #[schemars(length(min = 1, max = 20))]
    pub participants: Vec<String>,
    /// First local date in `YYYY-MM-DD` format.
    pub date_from: String,
    /// Last inclusive local date in `YYYY-MM-DD` format.
    pub date_to: String,
    /// IANA timezone such as `Europe/Belgrade`.
    pub time_zone: String,
    /// Explicit local-time windows to search.
    #[schemars(length(min = 1))]
    pub working_hours: Vec<WorkingHoursInput>,
    /// Meeting length from 15 through 480 minutes, divisible by 15.
    #[schemars(range(min = 15, max = 480))]
    pub duration_minutes: u16,
    /// Whether tentative intervals can be used; defaults to false.
    #[serde(default)]
    pub allow_tentative: bool,
    /// Per-participant role, timezone and working-hours overrides; omitted means all required.
    #[serde(default)]
    pub participant_options: Vec<super::CalendarParticipantOptions>,
    /// Break before and after each meeting: 0-120 minutes, divisible by 15. Default zero.
    #[serde(default)]
    #[schemars(range(min = 0, max = 120))]
    pub buffer_minutes: u16,
    /// Maximum windows and suggestions, default 20 and maximum 50; recurring uses 5/10.
    #[schemars(range(min = 1, max = 50))]
    pub limit: Option<u8>,
}

/// Input for compact own-calendar search or a bounded agenda range.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarSearchInput {
    /// Optional search text. Required when no date range is supplied.
    #[schemars(length(min = 1))]
    pub query: Option<String>,
    /// First local date in `YYYY-MM-DD` format.
    pub date_from: Option<String>,
    /// Last inclusive local date in `YYYY-MM-DD` format.
    pub date_to: Option<String>,
    /// IANA timezone used to interpret the optional local date range.
    pub time_zone: Option<String>,
    /// Account IDs; omitted means all enabled accounts.
    pub account_ids: Option<Vec<String>>,
    /// Maximum event summaries, default 50 and maximum 100.
    #[schemars(range(min = 1, max = 100))]
    pub limit: Option<u8>,
}

/// Input for one full Calendar item.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarGetInput {
    /// Process-local event reference returned by Calendar Search.
    pub event_ref: String,
    /// Requested body characters: default 12,000, maximum 50,000.
    #[schemars(range(min = 1, max = 50_000))]
    pub body_limit: Option<u32>,
}

/// Stable free/busy interval state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FreeBusyState {
    /// Free.
    Free,
    /// Tentative.
    Tentative,
    /// Busy.
    Busy,
    /// Out of office.
    OutOfOffice,
    /// Exchange has no data.
    NoData,
}

/// Stable participant resolution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantResolutionState {
    /// Exactly one directory recipient resolved.
    Resolved,
    /// Multiple complete suggestions were returned.
    Ambiguous,
    /// Suggestions are incomplete.
    AmbiguousPartial,
    /// No recipient matched.
    NotFound,
}

/// Availability state for a resolved participant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantAvailabilityState {
    /// Free/busy intervals are available.
    Available,
    /// Exchange rejected the recipient count.
    TooManyRecipients,
    /// A distribution list is too large.
    DistributionListTooLarge,
    /// Exchange reported a transient failure.
    TransientFailure,
    /// Exchange reported a permanent failure.
    Failure,
    /// Exchange omitted free/busy data.
    Missing,
}

/// One compact directory suggestion.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarRecipientCandidate {
    /// Directory display name.
    pub display_name: String,
    /// Directory email address.
    pub email: String,
    /// External content marker.
    pub untrusted_external_content: bool,
}

/// One merged free/busy interval in the requested timezone.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarAvailabilityInterval {
    /// Inclusive interval start with UTC offset.
    pub starts_at: String,
    /// Exclusive interval end with UTC offset.
    pub ends_at: String,
    /// Merged EAS status.
    pub status: FreeBusyState,
}

/// Resolution and compact schedule for one participant input.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarParticipantSchedule {
    /// Original caller input.
    pub input: String,
    /// Directory resolution state.
    pub resolution: ParticipantResolutionState,
    /// Resolved display name.
    pub display_name: Option<String>,
    /// Resolved email address.
    pub email: Option<String>,
    /// Total matching directory candidates.
    pub total_candidates: u32,
    /// Bounded suggestions when resolution is ambiguous.
    pub candidates: Vec<CalendarRecipientCandidate>,
    /// Availability state for an exact recipient.
    pub availability: ParticipantAvailabilityState,
    /// Merged intervals clipped to requested working hours.
    pub intervals: Vec<CalendarAvailabilityInterval>,
    /// External content marker.
    pub untrusted_external_content: bool,
}

/// Compact free/busy response.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarAvailabilityData {
    /// Account used for the directory request.
    pub account_id: String,
    /// Requested first date.
    pub date_from: String,
    /// Requested inclusive last date.
    pub date_to: String,
    /// Requested IANA timezone.
    pub time_zone: String,
    /// EAS free/busy precision.
    pub precision_minutes: u8,
    /// Whether every participant resolved exactly.
    pub resolution_complete: bool,
    /// Per-participant schedules.
    pub participants: Vec<CalendarParticipantSchedule>,
}

/// Participant summary returned with common windows.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarSlotParticipant {
    /// Original caller input.
    pub input: String,
    /// Directory resolution state.
    pub resolution: ParticipantResolutionState,
    /// Resolved display name.
    pub display_name: Option<String>,
    /// Resolved email address.
    pub email: Option<String>,
    /// Availability state.
    pub availability: ParticipantAvailabilityState,
    /// Bounded suggestions when resolution is ambiguous.
    pub candidates: Vec<CalendarRecipientCandidate>,
    /// Whether at least one requested interval had no data.
    pub has_no_data: bool,
    /// External content marker.
    pub untrusted_external_content: bool,
}

/// One contiguous common window that can fit the requested duration.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarFreeWindow {
    /// Earliest possible meeting start.
    pub window_start: String,
    /// End of the contiguous common-free interval.
    pub window_end: String,
    /// Latest possible meeting start for the requested duration.
    pub latest_start: String,
}

/// Common free-window response.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarSlotsData {
    /// Account used for the directory request.
    pub account_id: String,
    /// Requested IANA timezone.
    pub time_zone: String,
    /// Requested meeting duration.
    pub duration_minutes: u16,
    /// EAS free/busy precision.
    pub precision_minutes: u8,
    /// Whether every participant resolved exactly.
    pub resolution_complete: bool,
    /// Participant resolution summaries.
    pub participants: Vec<CalendarSlotParticipant>,
    /// Chronological common free windows.
    pub windows: Vec<CalendarFreeWindow>,
    /// Ranked concrete starts; required participants are conflict-free, optional conflicts explicit.
    pub suggestions: Vec<super::CalendarSlotSuggestion>,
    /// Requested break on each side of the meeting.
    pub buffer_minutes: u16,
    /// Whether additional valid windows or suggestions were omitted by the limit.
    pub results_truncated: bool,
}

/// Compact own-calendar Search result.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarEventSummary {
    /// Portable opaque event reference.
    pub event_ref: String,
    /// Owning account ID.
    pub account_id: String,
    /// Subject.
    pub subject: String,
    /// Start time in UTC.
    pub starts_at: Option<String>,
    /// End time in UTC.
    pub ends_at: Option<String>,
    /// All-day marker.
    pub all_day: bool,
    /// Location.
    pub location: String,
    /// Organizer.
    pub organizer: String,
    /// Number of attendees in the Search result.
    pub attendee_count: u32,
    /// Whether this summary represents an occurrence of a recurring series.
    pub recurring: bool,
    /// External content marker.
    pub untrusted_external_content: bool,
}

/// Bounded compact Calendar search or agenda response.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarSearchData {
    /// Compact matching events.
    pub items: Vec<CalendarEventSummary>,
    /// Whether Exchange reported more matching events.
    pub results_truncated: bool,
}

/// Full own-calendar event fetched on demand.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarEvent {
    /// Portable opaque event reference.
    pub event_ref: String,
    /// Owning account ID.
    pub account_id: String,
    /// Subject.
    pub subject: String,
    /// Sanitized plain-text body.
    pub body: String,
    /// Whether the body was truncated by Exchange or the local limit.
    pub body_truncated: bool,
    /// Start time in UTC.
    pub starts_at: Option<String>,
    /// End time in UTC.
    pub ends_at: Option<String>,
    /// All-day marker.
    pub all_day: bool,
    /// Location.
    pub location: String,
    /// Organizer.
    pub organizer: String,
    /// Organizer SMTP address.
    pub organizer_email: String,
    /// Stable iCalendar UID.
    pub uid: String,
    /// Personal, organizer, or attendee event classification.
    pub event_type: CalendarEventType,
    /// Current free/busy state.
    pub busy_status: CalendarBusyStatus,
    /// Current user's meeting response, when known.
    pub response_status: CalendarAttendeeStatus,
    /// Structured attendees.
    pub attendees: Vec<CalendarAttendeeView>,
    /// Recurrence fields from Exchange.
    pub recurrence: BTreeMap<String, String>,
    /// Recurrence exception fields.
    pub exceptions: Vec<BTreeMap<String, String>>,
    /// Whether `calendar_update` is allowed for this reference.
    pub can_update: bool,
    /// Whether `calendar_delete` is allowed for this reference.
    pub can_delete: bool,
    /// Whether `calendar_cancel` is allowed for this reference.
    pub can_cancel: bool,
    /// Whether `calendar_respond` is allowed for this reference.
    pub can_respond: bool,
    /// External content marker.
    pub untrusted_external_content: bool,
}
