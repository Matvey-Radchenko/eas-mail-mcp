use clap::{Args, Subcommand};

use super::common::{
    BodySource, BusyStatusArg, CommentSource, InputSource, ResponseArg, WriteControl,
};

#[derive(Debug, Subcommand)]
pub(in crate::cli) enum CalendarCommand {
    /// Resolve participants and return compact free/busy intervals.
    Availability(CalendarAvailabilityArgs),
    /// Find common free meeting windows.
    FindSlots(CalendarFindSlotsArgs),
    /// Search own-calendar event text.
    Search(CalendarSearchArgs),
    /// List a bounded own-calendar date range.
    Agenda(CalendarAgendaArgs),
    /// Fetch one full own-calendar event.
    Get(CalendarGetArgs),
    /// Create one event or meeting.
    Create(CalendarCreateArgs),
    /// Update one event or organizer meeting.
    Update(CalendarUpdateArgs),
    /// Delete one personal event.
    Delete(CalendarDeleteArgs),
    /// Cancel one organizer meeting.
    Cancel(CalendarCancelArgs),
    /// Respond to one received meeting.
    Respond(CalendarRespondArgs),
}

#[derive(Debug, Args)]
pub(in crate::cli) struct CalendarAvailabilityArgs {
    #[command(flatten)]
    pub(super) source: InputSource,
    /// Account used for the directory request.
    #[arg(long)]
    pub(super) account: Option<String>,
    /// Participant name or email; repeat for multiple participants.
    #[arg(long = "participant")]
    pub(super) participants: Vec<String>,
    /// First local date in YYYY-MM-DD format.
    #[arg(long = "from")]
    pub(super) date_from: Option<String>,
    /// Last inclusive local date in YYYY-MM-DD format.
    #[arg(long = "to")]
    pub(super) date_to: Option<String>,
    /// IANA timezone.
    #[arg(long)]
    pub(super) time_zone: Option<String>,
    /// Working interval such as mon,tue,wed,thu,fri@09:00-18:00; repeat as needed.
    #[arg(long = "working-hours")]
    pub(super) working_hours: Vec<String>,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct CalendarFindSlotsArgs {
    #[command(flatten)]
    pub(super) availability: CalendarAvailabilityArgs,
    /// Meeting length in minutes, from 15 through 480 and divisible by 15.
    #[arg(long)]
    pub(super) duration: Option<u16>,
    /// Permit tentative intervals.
    #[arg(long)]
    pub(super) allow_tentative: bool,
    /// Maximum windows, default 20 and maximum 50.
    #[arg(long)]
    pub(super) limit: Option<u8>,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct CalendarSearchArgs {
    /// Search text; omit only when using --input.
    pub(super) query: Option<String>,
    #[command(flatten)]
    pub(super) source: InputSource,
    /// Account ID; repeat to select multiple accounts.
    #[arg(long = "account")]
    pub(super) accounts: Vec<String>,
    /// Maximum results, default 50 and maximum 100.
    #[arg(long)]
    pub(super) limit: Option<u8>,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct CalendarAgendaArgs {
    #[command(flatten)]
    pub(super) source: InputSource,
    /// First local date in YYYY-MM-DD format.
    #[arg(long = "from")]
    pub(super) date_from: Option<String>,
    /// Last inclusive local date in YYYY-MM-DD format.
    #[arg(long = "to")]
    pub(super) date_to: Option<String>,
    /// IANA timezone.
    #[arg(long)]
    pub(super) time_zone: Option<String>,
    /// Account ID; repeat to select multiple accounts.
    #[arg(long = "account")]
    pub(super) accounts: Vec<String>,
    /// Maximum results, default 50 and maximum 100.
    #[arg(long)]
    pub(super) limit: Option<u8>,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct CalendarGetArgs {
    /// Portable event reference; omit only when using --input.
    pub(super) event_ref: Option<String>,
    #[command(flatten)]
    pub(super) source: InputSource,
    /// Maximum body characters, default 12,000 and maximum 50,000.
    #[arg(long)]
    pub(super) body_limit: Option<u32>,
}

#[derive(Debug, Args, Default)]
pub(super) struct ScheduleArgs {
    /// RFC3339 timed-event start.
    #[arg(long)]
    pub(super) start: Option<String>,
    /// RFC3339 timed-event exclusive end.
    #[arg(long)]
    pub(super) end: Option<String>,
    /// Inclusive all-day start date.
    #[arg(long, conflicts_with_all = ["start", "end"])]
    pub(super) all_day_start: Option<String>,
    /// Exclusive all-day end date.
    #[arg(long, conflicts_with_all = ["start", "end"])]
    pub(super) all_day_end: Option<String>,
    /// IANA timezone for the event schedule.
    #[arg(long)]
    pub(super) time_zone: Option<String>,
}

#[derive(Debug, Args, Default)]
pub(super) struct AttendeeArgs {
    /// Required attendee email; repeat for multiple attendees.
    #[arg(long)]
    pub(super) required: Vec<String>,
    /// Optional attendee email; repeat for multiple attendees.
    #[arg(long)]
    pub(super) optional: Vec<String>,
    /// Resource email; repeat for multiple resources.
    #[arg(long)]
    pub(super) resource: Vec<String>,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct CalendarCreateArgs {
    #[command(flatten)]
    pub(super) recurrence: super::calendar_recurrence::RecurrenceArgs,
    #[command(flatten)]
    pub(super) source: InputSource,
    /// Owning account ID.
    #[arg(long)]
    pub(super) account: Option<String>,
    /// Event subject.
    #[arg(long)]
    pub(super) subject: Option<String>,
    #[command(flatten)]
    pub(super) schedule: ScheduleArgs,
    #[command(flatten)]
    pub(super) content: BodySource,
    /// Event location.
    #[arg(long)]
    pub(super) location: Option<String>,
    /// Reminder in minutes before the event.
    #[arg(long)]
    pub(super) reminder: Option<u32>,
    /// Free/busy state, default busy.
    #[arg(long, value_enum)]
    pub(super) busy_status: Option<BusyStatusArg>,
    #[command(flatten)]
    pub(super) attendees: AttendeeArgs,
    #[command(flatten)]
    pub(super) control: WriteControl,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct CalendarUpdateArgs {
    /// Mutation boundary; required for recurring events.
    #[arg(long, value_enum)]
    pub(super) scope: Option<crate::model::CalendarScope>,
    #[command(flatten)]
    pub(super) recurrence: super::calendar_recurrence::RecurrenceArgs,
    /// Portable event reference; omit only when using --input.
    pub(super) event_ref: Option<String>,
    #[command(flatten)]
    pub(super) source: InputSource,
    /// Replacement subject.
    #[arg(long)]
    pub(super) subject: Option<String>,
    #[command(flatten)]
    pub(super) schedule: ScheduleArgs,
    #[command(flatten)]
    pub(super) content: BodySource,
    /// Replacement location.
    #[arg(long)]
    pub(super) location: Option<String>,
    /// Replacement reminder in minutes.
    #[arg(long, conflicts_with = "clear_reminder")]
    pub(super) reminder: Option<u32>,
    /// Remove the current reminder.
    #[arg(long)]
    pub(super) clear_reminder: bool,
    /// Replacement free/busy state.
    #[arg(long, value_enum)]
    pub(super) busy_status: Option<BusyStatusArg>,
    #[command(flatten)]
    pub(super) attendees: AttendeeArgs,
    /// Replace attendees with an empty list.
    #[arg(long, conflicts_with_all = ["required", "optional", "resource"])]
    pub(super) clear_attendees: bool,
    #[command(flatten)]
    pub(super) control: WriteControl,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct CalendarDeleteArgs {
    /// Mutation boundary; required for recurring events.
    #[arg(long, value_enum)]
    pub(super) scope: Option<crate::model::CalendarScope>,
    /// Portable event reference; omit only when using --input.
    pub(super) event_ref: Option<String>,
    #[command(flatten)]
    pub(super) source: InputSource,
    #[command(flatten)]
    pub(super) control: WriteControl,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct CalendarCancelArgs {
    /// Mutation boundary; required for recurring events.
    #[arg(long, value_enum)]
    pub(super) scope: Option<crate::model::CalendarScope>,
    /// Portable event reference; omit only when using --input.
    pub(super) event_ref: Option<String>,
    #[command(flatten)]
    pub(super) source: InputSource,
    #[command(flatten)]
    pub(super) content: CommentSource,
    #[command(flatten)]
    pub(super) control: WriteControl,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct CalendarRespondArgs {
    /// Mutation boundary; required for recurring events.
    #[arg(long, value_enum)]
    pub(super) scope: Option<crate::model::CalendarScope>,
    /// Portable event or meeting-request mail reference; omit only with --input.
    pub(super) event_ref: Option<String>,
    /// Response choice; omit only when using --input.
    #[arg(value_enum)]
    pub(super) response: Option<ResponseArg>,
    #[command(flatten)]
    pub(super) source: InputSource,
    #[command(flatten)]
    pub(super) content: CommentSource,
    #[command(flatten)]
    pub(super) control: WriteControl,
}
