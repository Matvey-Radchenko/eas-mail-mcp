use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Calendar free/busy state used when creating or updating an event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CalendarBusyStatus {
    /// The interval remains free.
    Free,
    /// The event is tentative.
    Tentative,
    /// The interval is busy.
    #[default]
    Busy,
    /// The owner is out of office.
    OutOfOffice,
}

/// Attendee role in a meeting invitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CalendarAttendeeRole {
    /// Required participant.
    Required,
    /// Optional participant.
    Optional,
    /// Room or other resource.
    Resource,
}

/// One attendee accepted by Calendar create and update tools.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarAttendeeInput {
    /// SMTP address.
    pub email: String,
    /// Optional display name.
    pub name: Option<String>,
    /// Required, optional, or resource role.
    pub role: CalendarAttendeeRole,
}

/// Timed or all-day schedule for a Calendar mutation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CalendarScheduleInput {
    /// RFC3339 instants whose offsets must agree with the IANA timezone.
    Timed {
        /// Inclusive event start.
        start: String,
        /// Exclusive event end.
        end: String,
        /// IANA timezone used to encode the EAS timezone structure.
        time_zone: String,
    },
    /// Date-only all-day interval with an exclusive end date.
    AllDay {
        /// Inclusive local start date in `YYYY-MM-DD` format.
        start_date: String,
        /// Exclusive local end date in `YYYY-MM-DD` format.
        end_date: String,
        /// IANA timezone used to resolve local midnights.
        time_zone: String,
    },
}

/// Input for creating a personal event or meeting, optionally recurring.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarCreateInput {
    /// Optional repeat rule; omitted for a one-off event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<super::CalendarRecurrenceInput>,
    /// Owning account ID.
    pub account_id: String,
    /// Event subject.
    #[schemars(length(min = 1, max = 998))]
    pub subject: String,
    /// Timed or date-only schedule.
    pub schedule: CalendarScheduleInput,
    /// Plain-text body, maximum 50,000 Unicode scalar values.
    #[serde(default)]
    #[schemars(length(max = 50_000))]
    pub body: String,
    /// Display location.
    #[serde(default)]
    pub location: String,
    /// Optional reminder in minutes before the event.
    pub reminder_minutes: Option<u32>,
    /// Free/busy state, default busy.
    #[serde(default)]
    pub busy_status: CalendarBusyStatus,
    /// Meeting attendees, maximum 100; empty creates a personal event.
    #[serde(default)]
    #[schemars(length(max = 100))]
    pub attendees: Vec<CalendarAttendeeInput>,
    /// UUID used for operation idempotency.
    pub idempotency_key: String,
}

/// Patch input for a personal event or organizer meeting with an explicit recurring scope.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarUpdateInput {
    /// Required for recurring events; omitted preserves one-off behavior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<super::CalendarScope>,
    /// Replacement repeat rule for series or following; omitted preserves it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<super::CalendarRecurrenceInput>,
    /// Process-local reference returned by Calendar Search or create.
    pub event_ref: String,
    /// Replacement subject; an empty value is rejected.
    #[schemars(length(max = 998))]
    pub subject: Option<String>,
    /// Replacement schedule.
    pub schedule: Option<CalendarScheduleInput>,
    /// Replacement plain-text body; an empty string clears it.
    #[schemars(length(max = 50_000))]
    pub body: Option<String>,
    /// Replacement location; an empty string clears it.
    pub location: Option<String>,
    /// Replacement reminder in minutes.
    pub reminder_minutes: Option<u32>,
    /// Explicitly removes the reminder; mutually exclusive with `reminder_minutes`.
    #[serde(default)]
    pub clear_reminder: bool,
    /// Replacement free/busy state.
    pub busy_status: Option<CalendarBusyStatus>,
    /// Complete replacement attendee list; an empty list makes the event personal.
    #[schemars(length(max = 100))]
    pub attendees: Option<Vec<CalendarAttendeeInput>>,
    /// UUID used for operation idempotency.
    pub idempotency_key: String,
}

/// Input for deleting a personal event.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarDeleteInput {
    /// Required for recurring events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<super::CalendarScope>,
    /// Process-local event reference.
    pub event_ref: String,
    /// UUID used for operation idempotency.
    pub idempotency_key: String,
}

/// Input for cancelling an organizer meeting.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarCancelInput {
    /// Required for recurring meetings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<super::CalendarScope>,
    /// Process-local event reference.
    pub event_ref: String,
    /// Optional plain-text cancellation comment.
    #[serde(default)]
    #[schemars(length(max = 50_000))]
    pub comment: String,
    /// UUID used for operation idempotency.
    pub idempotency_key: String,
}

/// Meeting response choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CalendarResponseChoice {
    /// Accept the meeting.
    Accept,
    /// Tentatively accept the meeting.
    Tentative,
    /// Decline the meeting.
    Decline,
}

/// Input for responding to a received meeting.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarRespondInput {
    /// Required for recurring meetings; following is not supported for responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<super::CalendarScope>,
    /// Process-local Calendar event or actionable meeting-request mail reference.
    pub event_ref: String,
    /// Accept, tentatively accept, or decline.
    pub response: CalendarResponseChoice,
    /// Optional plain-text reply comment.
    #[serde(default)]
    #[schemars(length(max = 50_000))]
    pub comment: String,
    /// UUID used for operation idempotency.
    pub idempotency_key: String,
}

/// Calendar event ownership classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CalendarEventType {
    /// Event without attendees.
    Personal,
    /// Meeting owned by this account.
    OrganizerMeeting,
    /// Meeting received by this account.
    AttendeeMeeting,
}

/// Participation status returned for one attendee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CalendarAttendeeStatus {
    /// No response was recorded.
    NoResponse,
    /// Attendee tentatively accepted.
    Tentative,
    /// Attendee accepted.
    Accepted,
    /// Attendee declined.
    Declined,
    /// Exchange returned an unrecognized status.
    Unknown,
}

/// Structured attendee returned by `calendar_get`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarAttendeeView {
    /// SMTP address.
    pub email: String,
    /// Display name.
    pub name: String,
    /// Meeting role.
    pub role: CalendarAttendeeRole,
    /// Current participation status.
    pub status: CalendarAttendeeStatus,
    /// External content marker.
    pub untrusted_external_content: bool,
}

/// Stable state of a Calendar lifecycle operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CalendarOperationState {
    /// Every required step succeeded.
    Succeeded,
    /// No external step succeeded and Exchange rejected the operation safely.
    Failed,
    /// Some confirmed steps succeeded and a later step failed safely.
    Partial,
    /// A network failure left at least one step's outcome unknown.
    Unknown,
}

/// Result of an idempotent Calendar lifecycle operation.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CalendarOperationResult {
    /// UUID supplied by the caller.
    pub operation_id: String,
    /// Final, partial, or unknown state.
    pub status: CalendarOperationState,
    /// Stable names of confirmed completed steps.
    pub completed_steps: Vec<String>,
    /// Safe status text.
    pub message: String,
    /// New portable reference when a resulting event remains available.
    pub event_ref: Option<String>,
}
