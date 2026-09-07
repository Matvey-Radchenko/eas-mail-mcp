use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Kind of synchronized EAS collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionKind {
    /// Mail folder.
    Mail,
    /// Calendar folder.
    Calendar,
}

/// Field-presence marker used by partial EAS Change commands.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub enum Patch<T> {
    /// The server did not include this field.
    #[default]
    Missing,
    /// The server explicitly supplied this value, including an empty value.
    Value(T),
}

/// EAS folder metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    /// Server folder identifier.
    pub server_id: String,
    /// Parent folder identifier.
    pub parent_id: String,
    /// Display name supplied by Exchange.
    pub display_name: String,
    /// Numeric EAS folder type.
    pub folder_type: u16,
    /// Collection kind supported by this client.
    pub kind: Option<CollectionKind>,
}

/// Result of one FolderSync response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderPage {
    /// FolderSync status.
    pub status: u16,
    /// New FolderSync key.
    pub sync_key: String,
    /// Added or updated folders.
    pub folders: Vec<Folder>,
    /// Deleted folder identifiers.
    pub deleted_ids: Vec<String>,
}

/// Attachment metadata returned with a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// Safe display name.
    pub display_name: String,
    /// Opaque Exchange file reference.
    pub file_reference: String,
    /// Estimated payload size in bytes.
    pub size: u64,
    /// MIME content type.
    pub content_type: String,
    /// Whether the attachment is inline.
    pub is_inline: bool,
    /// Optional inline content identifier.
    pub content_id: String,
}

/// Meeting request metadata embedded in an EAS email item.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MeetingRequest {
    /// Whether the meeting covers whole local dates.
    pub all_day: bool,
    /// Organizer-supplied creation or update timestamp.
    pub dt_stamp: Option<DateTime<Utc>>,
    /// UTC meeting start.
    pub starts_at: Option<DateTime<Utc>>,
    /// UTC exclusive meeting end.
    pub ends_at: Option<DateTime<Utc>>,
    /// EAS instance type; only zero is non-recurring.
    pub instance_type: u8,
    /// Display location.
    pub location: String,
    /// Organizer address header.
    pub organizer: String,
    /// Optional reminder in minutes.
    pub reminder_minutes: Option<u32>,
    /// Whether the organizer requests a response.
    pub response_requested: bool,
    /// EAS free/busy status.
    pub busy_status: u8,
    /// Base64-encoded EAS timezone structure.
    pub time_zone: String,
    /// Base64-encoded Exchange global object identifier.
    pub global_object_id: String,
    /// Calendar UID supplied directly by newer protocol versions.
    pub uid: String,
    /// EAS 14.1 meeting message type.
    pub message_type: u8,
}

/// Mail fields with exact partial-update semantics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MailFields {
    /// Message subject.
    pub subject: Patch<String>,
    /// Sender header.
    pub sender: Patch<String>,
    /// To header.
    pub recipients: Patch<String>,
    /// Cc header.
    pub cc: Patch<String>,
    /// Server receive time.
    pub received_at: Patch<Option<DateTime<Utc>>>,
    /// Plain-text body or preview.
    pub body: Patch<String>,
    /// Whether Exchange truncated the body.
    pub body_truncated: Patch<bool>,
    /// Read state.
    pub is_read: Patch<bool>,
    /// EAS importance value.
    pub importance: Patch<u8>,
    /// Attachment list.
    pub attachments: Patch<Vec<Attachment>>,
    /// Exchange message class hint.
    pub message_class: Patch<String>,
    /// Meeting request metadata when this mail is actionable Calendar content.
    pub meeting_request: Patch<MeetingRequest>,
    /// Server-owned opaque conversation identifier; never interpreted as UTF-8.
    pub conversation_id: Patch<Vec<u8>>,
    /// Server-owned opaque position within a conversation.
    pub conversation_index: Patch<Vec<u8>>,
    /// Complete server flag container, retained for lossless flag changes.
    pub flag: Patch<crate::wbxml::Element>,
    /// Exact category set, with missing metadata distinct from an empty set.
    pub categories: Patch<Vec<String>>,
}

/// Calendar fields with exact partial-update semantics.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct CalendarFields {
    /// Lossless write metadata; absent only on legacy or partial projections.
    pub properties: Option<crate::CalendarProperties>,
    /// Event subject.
    pub subject: Patch<String>,
    /// Event body converted later by the application layer.
    pub body: Patch<String>,
    /// Whether Exchange truncated the event body.
    pub body_truncated: Patch<bool>,
    /// Start time.
    pub starts_at: Patch<Option<DateTime<Utc>>>,
    /// End time.
    pub ends_at: Patch<Option<DateTime<Utc>>>,
    /// All-day marker.
    pub all_day: Patch<bool>,
    /// Display location.
    pub location: Patch<String>,
    /// Organizer display name or address.
    pub organizer: Patch<String>,
    /// Organizer address used by meeting lifecycle operations.
    pub organizer_email: Patch<String>,
    /// Meeting attendees and their EAS roles and statuses.
    pub attendees: Patch<Vec<CalendarAttendee>>,
    /// Reminder in minutes; an explicit `None` clears an existing reminder.
    pub reminder_minutes: Patch<Option<u32>>,
    /// Recurrence fields retained for read-only clients.
    pub recurrence: Patch<BTreeMap<String, String>>,
    /// Exception fields retained for read-only clients.
    pub exceptions: Patch<Vec<BTreeMap<String, String>>>,
    /// EAS meeting status.
    pub meeting_status: Patch<u16>,
    /// Stable iCalendar UID.
    pub uid: Patch<String>,
    /// Last modification timestamp supplied by Exchange.
    pub dt_stamp: Patch<Option<DateTime<Utc>>>,
    /// Base64-encoded EAS timezone blob.
    pub time_zone: Patch<String>,
    /// EAS free/busy status.
    pub busy_status: Patch<u8>,
    /// Whether the organizer requests attendee responses.
    pub response_requested: Patch<bool>,
    /// Current user's EAS meeting response type.
    pub response_type: Patch<u8>,
}

/// One Calendar attendee parsed from or sent to Exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CalendarAttendee {
    /// SMTP address.
    pub email: String,
    /// Optional display name.
    pub name: String,
    /// EAS attendee role code: required, optional, or resource.
    pub attendee_type: u8,
    /// EAS participation status code.
    pub attendee_status: u8,
}

/// Complete non-recurring Calendar item used for Add and Change commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarApplication {
    /// Recurrence, overrides and other preserved Calendar properties.
    pub properties: crate::CalendarProperties,
    /// Base64-encoded 172-byte EAS timezone structure.
    pub time_zone: String,
    /// Stable iCalendar UID.
    pub uid: String,
    /// UTC modification timestamp.
    pub dt_stamp: DateTime<Utc>,
    /// UTC event start.
    pub starts_at: DateTime<Utc>,
    /// UTC exclusive event end.
    pub ends_at: DateTime<Utc>,
    /// All-day marker.
    pub all_day: bool,
    /// Event subject.
    pub subject: String,
    /// Plain-text body.
    pub body: String,
    /// Display location.
    pub location: String,
    /// Optional reminder in minutes.
    pub reminder_minutes: Option<u32>,
    /// EAS free/busy status.
    pub busy_status: u8,
    /// EAS meeting status.
    pub meeting_status: u16,
    /// Whether responses are requested.
    pub response_requested: bool,
    /// Meeting attendees.
    pub attendees: Vec<CalendarAttendee>,
}

/// Payload of an EAS Sync change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeData {
    /// No application data, normally for deletion.
    None,
    /// Mail application data.
    Mail(MailFields),
    /// Calendar application data.
    Calendar(CalendarFields),
}

/// EAS change command kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// New server item.
    Add,
    /// Partial update to an existing item.
    Change,
    /// Hard deletion.
    Delete,
    /// Soft deletion outside the synchronization window.
    SoftDelete,
}

/// Ordered item change from one Sync page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncChange {
    /// Change kind.
    pub kind: ChangeKind,
    /// Server item identifier.
    pub server_id: String,
    /// Optional application data.
    pub data: ChangeData,
}

/// One parsed EAS Sync response page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPage {
    /// Top-level Sync status.
    pub account_status: u16,
    /// Collection status.
    pub collection_status: u16,
    /// New collection SyncKey.
    pub sync_key: String,
    /// Whether another page must be requested.
    pub more_available: bool,
    /// Ordered server changes.
    pub changes: Vec<SyncChange>,
}

/// One server-side mailbox search result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMail {
    /// LongId used by ItemOperations.
    pub long_id: String,
    /// Collection identifier when supplied by Exchange.
    pub collection_id: Option<String>,
    /// Mutable item identifier when supplied by Exchange.
    pub server_id: Option<String>,
    /// Parsed summary fields.
    pub fields: MailFields,
}

/// One server-side calendar search result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchCalendar {
    /// LongId used by ItemOperations.
    pub long_id: String,
    /// Calendar collection identifier when supplied by Exchange.
    pub collection_id: Option<String>,
    /// Calendar server identifier when supplied by Exchange.
    pub server_id: Option<String>,
    /// Parsed summary fields.
    pub fields: CalendarFields,
}

/// Bounded server-side calendar search page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchCalendarPage {
    /// Calendar items returned by Exchange.
    pub items: Vec<SearchCalendar>,
    /// Total matches reported by Exchange.
    pub total: usize,
}

/// Full item returned by ItemOperations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemResult {
    /// Collection identifier when supplied by Exchange.
    pub collection_id: Option<String>,
    /// Mutable item identifier when supplied by Exchange.
    pub server_id: Option<String>,
    /// Parsed mail fields.
    pub fields: MailFields,
}

/// Full calendar item returned by ItemOperations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarItemResult {
    /// Calendar collection identifier when supplied by Exchange.
    pub collection_id: Option<String>,
    /// Calendar server identifier when supplied by Exchange.
    pub server_id: Option<String>,
    /// Parsed calendar fields.
    pub fields: CalendarFields,
}

/// Directory resolution state returned by ResolveRecipients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipientResolution {
    /// Exactly one recipient resolved.
    Resolved,
    /// Multiple complete suggestions were returned.
    Ambiguous,
    /// The suggestions are only a partial result set.
    AmbiguousPartial,
    /// No directory recipient matched.
    NotFound,
}

/// One 30-minute status from EAS MergedFreeBusy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreeBusyStatus {
    /// No appointment blocks the interval.
    Free,
    /// A tentative appointment overlaps the interval.
    Tentative,
    /// A busy appointment overlaps the interval.
    Busy,
    /// An out-of-office appointment overlaps the interval.
    OutOfOffice,
    /// Exchange has no availability data for the interval.
    NoData,
}

/// Availability state for one resolved directory candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateAvailability {
    /// Complete free/busy slots were returned.
    Slots(Vec<FreeBusyStatus>),
    /// Exchange rejected the number of exact recipients.
    TooManyRecipients,
    /// A distribution list was too large to expand.
    DistributionListTooLarge,
    /// Availability failed temporarily.
    TransientFailure,
    /// Availability failed permanently.
    Failure,
    /// No Availability element was returned.
    Missing,
}

/// One directory candidate from ResolveRecipients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRecipient {
    /// EAS recipient type code.
    pub recipient_type: u16,
    /// Directory display name.
    pub display_name: String,
    /// Directory email address.
    pub email: String,
    /// Free/busy result for this candidate.
    pub availability: CandidateAvailability,
}

/// Resolution and availability for one supplied participant value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipientAvailability {
    /// Original value sent in the To element.
    pub input: String,
    /// Resolution result.
    pub resolution: RecipientResolution,
    /// Total candidate count reported by Exchange.
    pub total_candidates: usize,
    /// Bounded directory candidates.
    pub candidates: Vec<ResolvedRecipient>,
}

/// Result of a mail mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationResult {
    /// EAS operation status.
    pub status: u16,
    /// New collection SyncKey when applicable.
    pub sync_key: Option<String>,
    /// Server item identifier when applicable.
    pub server_id: Option<String>,
}

/// Response choice encoded by the EAS MeetingResponse command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingResponseChoice {
    /// Accept the meeting.
    Accept,
    /// Tentatively accept the meeting.
    Tentative,
    /// Decline the meeting.
    Decline,
}

impl MeetingResponseChoice {
    /// Returns the EAS UserResponse numeric value.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Accept => 1,
            Self::Tentative => 2,
            Self::Decline => 3,
        }
    }
}

/// Parsed EAS MeetingResponse result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingResponseResult {
    /// Command status.
    pub status: u16,
    /// Request identifier echoed by Exchange.
    pub request_id: String,
    /// New Calendar server identifier for accepted or tentative meetings.
    pub calendar_id: Option<String>,
}
