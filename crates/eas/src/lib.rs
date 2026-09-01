//! Exchange ActiveSync protocol and transport primitives.

#![deny(missing_docs)]

mod calendar;
mod calendar_projection;
mod client;
mod device;
mod error;
mod model;
mod profile;
pub mod protocol;
mod query;
mod transport;
pub mod wbxml;

pub use calendar::{
    CalendarException, CalendarProperties, CalendarRecurrence, RecurrenceEnd, RecurrencePattern,
};
pub use client::{EasClient, NegotiatedPolicy, ServerCapabilities};
pub use eas_mail_profile::IdentityMode;
pub use error::{EasError, Result};
pub use model::{
    Attachment, CalendarApplication, CalendarAttendee, CalendarFields, CalendarItemResult,
    CandidateAvailability, ChangeData, ChangeKind, CollectionKind, Folder, FolderPage,
    FreeBusyStatus, ItemResult, MailFields, MeetingRequest, MeetingResponseChoice,
    MeetingResponseResult, MutationResult, Patch, RecipientAvailability, RecipientResolution,
    ResolvedRecipient, SearchCalendar, SearchCalendarPage, SearchMail, SyncChange, SyncPage,
};
pub use profile::{Profile, ProfileKey, ProfileRegistry};
pub use query::{Command, build_binary_query};
pub use transport::{HttpTransport, RequestSafety, Transport, TransportResponse};
