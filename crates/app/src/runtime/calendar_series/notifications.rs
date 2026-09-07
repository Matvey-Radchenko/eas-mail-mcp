use eas_mail_protocol::CalendarAttendee;

use super::edit::item_attendees;
use crate::runtime::calendar_prepare::PreparedEvent;

pub(super) fn recipients(old: &PreparedEvent, new: &PreparedEvent) -> Vec<CalendarAttendee> {
    let before = &old.mutation.application;
    let after = &new.mutation.application;
    let mut comparable = before.clone();
    comparable.attendees.clone_from(&after.attendees);
    comparable.dt_stamp = after.dt_stamp;
    comparable.meeting_status = after.meeting_status;
    comparable.response_requested = after.response_requested;
    if comparable != *after {
        // Schedule, content, recurrence and exception edits affect current attendees.
        return item_attendees(new);
    }
    // A roster-only update also notifies existing people whose role changed, and
    // people promoted from an exception-only invitation to the entire series.
    after
        .attendees
        .iter()
        .filter(|attendee| {
            !before.attendees.iter().any(|prior| {
                prior.email.eq_ignore_ascii_case(&attendee.email)
                    && prior.attendee_type == attendee.attendee_type
            })
        })
        .cloned()
        .collect()
}
