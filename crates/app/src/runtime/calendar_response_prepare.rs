use chrono::{DateTime, Utc};
use eas_mail_protocol::{
    CalendarApplication, CalendarAttendee, MeetingRequest, Patch, protocol::global_object_id_uid,
};

use super::calendar_prepare::PreparedEvent;
use crate::backend::{BackendCalendarMutation, BackendMail};
use crate::sanitize::{mailbox, plain_text};
use crate::{AppError, ErrorCode, Result};

pub(super) struct PreparedMeetingRequest {
    pub(super) event: PreparedEvent,
    pub(super) organizer: CalendarAttendee,
}

pub(super) fn prepare(mail: &BackendMail, now: DateTime<Utc>) -> Result<PreparedMeetingRequest> {
    let request = request(&mail.fields.meeting_request)?;
    let message_class = string(&mail.fields.message_class);
    if !message_class.to_ascii_lowercase().contains(".meeting.request")
        || !matches!(request.message_type, 1 | 2)
        || !request.response_requested
    {
        return Err(validation("mail reference is not an actionable meeting request"));
    }
    if request.instance_type != 0 {
        return Err(validation("recurring events and recurrence exceptions are read-only"));
    }
    let starts_at =
        request.starts_at.ok_or_else(|| protocol("meeting request start is missing"))?;
    let ends_at = request.ends_at.ok_or_else(|| protocol("meeting request end is missing"))?;
    if starts_at >= ends_at {
        return Err(protocol("meeting request time range is invalid"));
    }
    let organizer = organizer(request, string(&mail.fields.sender))?;
    let uid = if request.uid.is_empty() {
        global_object_id_uid(&request.global_object_id).map_err(AppError::from)?
    } else {
        request.uid.clone()
    };
    let all_day_dates = request.all_day.then_some((starts_at.date_naive(), ends_at.date_naive()));
    Ok(PreparedMeetingRequest {
        event: PreparedEvent {
            mutation: BackendCalendarMutation {
                target_collection: None,
                application: CalendarApplication {
                    properties: Default::default(),
                    time_zone: request.time_zone.clone(),
                    uid,
                    dt_stamp: request.dt_stamp.unwrap_or(now),
                    starts_at,
                    ends_at,
                    all_day: request.all_day,
                    subject: plain_text(string(&mail.fields.subject)),
                    body: plain_text(string(&mail.fields.body)),
                    location: plain_text(&request.location),
                    reminder_minutes: request.reminder_minutes,
                    busy_status: request.busy_status.min(3),
                    meeting_status: 3,
                    response_requested: true,
                    attendees: Vec::new(),
                },
            },
            all_day_dates,
        },
        organizer,
    })
}

fn organizer(request: &MeetingRequest, fallback: &str) -> Result<CalendarAttendee> {
    let source = if request.organizer.trim().is_empty() { fallback } else { &request.organizer };
    let email = mailbox(source);
    if !valid_email(&email) {
        return Err(protocol("meeting request organizer is invalid"));
    }
    let name = source
        .rfind('<')
        .and_then(|index| source.get(..index))
        .map(|value| value.trim().trim_matches('"'))
        .map_or_else(String::new, plain_text);
    Ok(CalendarAttendee { email, name, attendee_type: 1, attendee_status: 0 })
}

fn request(value: &Patch<MeetingRequest>) -> Result<&MeetingRequest> {
    match value {
        Patch::Value(value) => Ok(value),
        Patch::Missing => Err(validation("mail reference has no meeting request metadata")),
    }
}

fn string(value: &Patch<String>) -> &str {
    match value {
        Patch::Value(value) => value,
        Patch::Missing => "",
    }
}

fn valid_email(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(char::is_control)
        && value.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        })
}

fn validation(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}

fn protocol(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ProtocolError, message)
}
