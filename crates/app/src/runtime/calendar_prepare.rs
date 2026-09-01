use std::collections::BTreeSet;

use chrono::{DateTime, NaiveDate, Utc};
use eas_mail_protocol::{CalendarApplication, CalendarAttendee, Patch};

use super::calendar_schedule;
use crate::backend::{BackendCalendarMutation, BackendEvent};
use crate::model::{
    CalendarAttendeeInput, CalendarAttendeeRole, CalendarBusyStatus, CalendarCreateInput,
    CalendarUpdateInput, MAX_OUTGOING_BODY_CHARS,
};
use crate::sanitize::plain_text;
use crate::{AppError, ErrorCode, Result};

#[derive(Clone)]
pub(super) struct PreparedEvent {
    pub(super) mutation: BackendCalendarMutation,
    pub(super) all_day_dates: Option<(NaiveDate, NaiveDate)>,
}

pub(super) struct PreparedUpdate {
    pub(super) event: PreparedEvent,
    pub(super) removed_attendees: Vec<CalendarAttendee>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EventOwnership {
    Personal,
    Organizer,
    Attendee,
}

pub(super) fn create(
    input: &CalendarCreateInput,
    now: DateTime<Utc>,
    uid: String,
    account_email: &str,
) -> Result<PreparedEvent> {
    validate_text(&input.subject, 1, 998, "calendar subject")?;
    validate_text(&input.body, 0, MAX_OUTGOING_BODY_CHARS, "calendar body")?;
    validate_text(&input.location, 0, 998, "calendar location")?;
    let schedule = calendar_schedule::prepare(&input.schedule)?;
    let attendees = attendees(&input.attendees, account_email)?;
    let is_meeting = !attendees.is_empty();
    let mut prepared = PreparedEvent {
        mutation: BackendCalendarMutation {
            target_collection: None,
            application: CalendarApplication {
                properties: Default::default(),
                time_zone: schedule.time_zone,
                uid,
                dt_stamp: now,
                starts_at: schedule.starts_at,
                ends_at: schedule.ends_at,
                all_day: schedule.all_day_dates.is_some(),
                subject: input.subject.clone(),
                body: input.body.clone(),
                location: input.location.clone(),
                reminder_minutes: input.reminder_minutes,
                busy_status: busy_code(input.busy_status),
                meeting_status: u16::from(is_meeting),
                response_requested: is_meeting,
                attendees,
            },
        },
        all_day_dates: schedule.all_day_dates,
    };
    if let Some(rule) = &input.recurrence {
        prepared.mutation.application.properties.recurrence =
            Some(super::calendar_series::rule::prepare(rule, &prepared.mutation.application)?);
    }
    super::calendar_series::validate(&prepared.mutation.application)?;
    Ok(prepared)
}

pub(super) fn update(
    input: &CalendarUpdateInput,
    source: &BackendEvent,
    now: DateTime<Utc>,
    account_email: &str,
) -> Result<PreparedUpdate> {
    if ownership(source, account_email) == EventOwnership::Attendee {
        return Err(validation("only the organizer can update this meeting"));
    }
    if input.clear_reminder && input.reminder_minutes.is_some() {
        return Err(validation("clear_reminder and reminder_minutes are mutually exclusive"));
    }
    if let Some(subject) = &input.subject {
        validate_text(subject, 1, 998, "calendar subject")?;
    }
    if let Some(body) = &input.body {
        validate_text(body, 0, MAX_OUTGOING_BODY_CHARS, "calendar body")?;
    }
    if let Some(location) = &input.location {
        validate_text(location, 0, 998, "calendar location")?;
    }
    let existing = existing(source, now)?;
    let schedule = input.schedule.as_ref().map(calendar_schedule::prepare).transpose()?;
    let replacement =
        input.attendees.as_ref().map(|values| attendees(values, account_email)).transpose()?;
    let old_attendees = existing.mutation.application.attendees.clone();
    let current_attendees = replacement.unwrap_or_else(|| old_attendees.clone());
    let removed_attendees = removed(&old_attendees, &current_attendees);
    let current_is_meeting = !current_attendees.is_empty()
        || existing.mutation.application.properties.has_attendee_overrides();
    let application = CalendarApplication {
        properties: existing.mutation.application.properties.clone(),
        time_zone: schedule.as_ref().map_or_else(
            || existing.mutation.application.time_zone.clone(),
            |value| value.time_zone.clone(),
        ),
        uid: existing.mutation.application.uid.clone(),
        dt_stamp: now,
        starts_at: schedule
            .as_ref()
            .map_or(existing.mutation.application.starts_at, |value| value.starts_at),
        ends_at: schedule
            .as_ref()
            .map_or(existing.mutation.application.ends_at, |value| value.ends_at),
        all_day: schedule
            .as_ref()
            .map_or(existing.mutation.application.all_day, |value| value.all_day_dates.is_some()),
        subject: input.subject.clone().unwrap_or(existing.mutation.application.subject),
        body: input.body.clone().unwrap_or(existing.mutation.application.body),
        location: input.location.clone().unwrap_or(existing.mutation.application.location),
        reminder_minutes: if input.clear_reminder {
            None
        } else {
            input.reminder_minutes.or(existing.mutation.application.reminder_minutes)
        },
        busy_status: input.busy_status.map_or(existing.mutation.application.busy_status, busy_code),
        meeting_status: u16::from(current_is_meeting),
        response_requested: current_is_meeting,
        attendees: current_attendees,
    };
    Ok(PreparedUpdate {
        event: PreparedEvent {
            mutation: BackendCalendarMutation { target_collection: None, application },
            all_day_dates: schedule.map_or(existing.all_day_dates, |value| value.all_day_dates),
        },
        removed_attendees,
    })
}

pub(super) fn existing(source: &BackendEvent, now: DateTime<Utc>) -> Result<PreparedEvent> {
    from_fields(source, now, super::calendar_series::properties(source)?)
}

pub(super) fn from_fields(
    source: &BackendEvent,
    now: DateTime<Utc>,
    properties: eas_mail_protocol::CalendarProperties,
) -> Result<PreparedEvent> {
    let fields = &source.fields;
    let starts_at = required_datetime(&fields.starts_at, "Calendar start is missing")?;
    let ends_at = required_datetime(&fields.ends_at, "Calendar end is missing")?;
    let all_day = boolean(&fields.all_day);
    let time_zone = required_string(&fields.time_zone, "Calendar timezone is missing")?;
    let all_day_dates = if all_day {
        let zone = super::calendar_agenda::timezone::EventTimeZone::parse(
            Some(&time_zone),
            chrono_tz::UTC,
        )?;
        Some((zone.to_local(starts_at)?.date(), zone.to_local(ends_at)?.date()))
    } else {
        None
    };
    Ok(PreparedEvent {
        mutation: BackendCalendarMutation {
            target_collection: None,
            application: CalendarApplication {
                properties,
                time_zone,
                uid: required_string(&fields.uid, "Calendar UID is missing")?,
                dt_stamp: now,
                starts_at,
                ends_at,
                all_day,
                subject: plain_text(string(&fields.subject)),
                body: plain_text(string(&fields.body)),
                location: plain_text(string(&fields.location)),
                reminder_minutes: optional_number(&fields.reminder_minutes),
                busy_status: number(&fields.busy_status),
                meeting_status: number(&fields.meeting_status),
                response_requested: boolean(&fields.response_requested),
                attendees: list(&fields.attendees),
            },
        },
        all_day_dates,
    })
}

pub(super) fn ownership(source: &BackendEvent, account_email: &str) -> EventOwnership {
    let exception_attendees = source
        .fields
        .properties
        .as_ref()
        .is_some_and(eas_mail_protocol::CalendarProperties::has_attendee_overrides);
    if list(&source.fields.attendees).is_empty()
        && !exception_attendees
        && number(&source.fields.meeting_status) & 1 == 0
    {
        EventOwnership::Personal
    } else if string(&source.fields.organizer_email).eq_ignore_ascii_case(account_email)
        || number(&source.fields.meeting_status) == 1
    {
        EventOwnership::Organizer
    } else {
        EventOwnership::Attendee
    }
}

pub(super) fn refresh_organizer_status(item: &mut CalendarApplication) {
    let meeting = !item.attendees.is_empty() || item.properties.has_attendee_overrides();
    if !meeting {
        item.response_requested = false;
    } else if item.meeting_status & 1 == 0 {
        item.response_requested = true;
    }
    item.meeting_status = u16::from(meeting);
}

pub(super) fn validate_comment(value: &str) -> Result<()> {
    validate_text(value, 0, MAX_OUTGOING_BODY_CHARS, "calendar comment")
}

fn attendees(
    values: &[CalendarAttendeeInput],
    account_email: &str,
) -> Result<Vec<CalendarAttendee>> {
    if values.len() > 100 {
        return Err(validation("calendar attendees exceed the 100-recipient limit"));
    }
    let mut seen = BTreeSet::new();
    let mut output = Vec::with_capacity(values.len());
    for value in values {
        let email = value.email.trim();
        validate_email(email)?;
        if email.eq_ignore_ascii_case(account_email) {
            return Err(validation("the organizer must not be listed as an attendee"));
        }
        if !seen.insert(email.to_ascii_lowercase()) {
            return Err(validation("calendar attendee addresses must be unique"));
        }
        let name = value.name.as_deref().unwrap_or_default().trim().to_owned();
        validate_text(&name, 0, 998, "calendar attendee name")?;
        output.push(CalendarAttendee {
            email: email.to_owned(),
            name,
            attendee_type: role_code(value.role),
            attendee_status: 0,
        });
    }
    Ok(output)
}

fn removed(old: &[CalendarAttendee], new: &[CalendarAttendee]) -> Vec<CalendarAttendee> {
    old.iter()
        .filter(|candidate| {
            !new.iter().any(|value| value.email.eq_ignore_ascii_case(&candidate.email))
        })
        .cloned()
        .collect()
}

fn validate_email(value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && !value.chars().any(char::is_control)
        && value.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        });
    if valid { Ok(()) } else { Err(validation("calendar attendee email is invalid")) }
}

fn validate_text(value: &str, min: usize, max: usize, name: &str) -> Result<()> {
    let length = value.chars().count();
    if length < min || length > max {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            format!("{name} must contain {min}-{max} Unicode characters"),
        ));
    }
    if value.chars().any(|value| value == '\0') {
        return Err(validation("calendar text contains a NUL character"));
    }
    Ok(())
}

const fn role_code(value: CalendarAttendeeRole) -> u8 {
    match value {
        CalendarAttendeeRole::Required => 1,
        CalendarAttendeeRole::Optional => 2,
        CalendarAttendeeRole::Resource => 3,
    }
}

const fn busy_code(value: CalendarBusyStatus) -> u8 {
    match value {
        CalendarBusyStatus::Free => 0,
        CalendarBusyStatus::Tentative => 1,
        CalendarBusyStatus::Busy => 2,
        CalendarBusyStatus::OutOfOffice => 3,
    }
}

fn required_string(value: &Patch<String>, message: &'static str) -> Result<String> {
    match value {
        Patch::Value(value) if !value.is_empty() => Ok(value.clone()),
        _ => Err(AppError::new(ErrorCode::ProtocolError, message)),
    }
}

fn required_datetime(
    value: &Patch<Option<DateTime<Utc>>>,
    message: &'static str,
) -> Result<DateTime<Utc>> {
    match value {
        Patch::Value(Some(value)) => Ok(*value),
        _ => Err(AppError::new(ErrorCode::ProtocolError, message)),
    }
}

fn string(value: &Patch<String>) -> &str {
    match value {
        Patch::Value(value) => value,
        Patch::Missing => "",
    }
}

fn boolean(value: &Patch<bool>) -> bool {
    matches!(value, Patch::Value(true))
}

fn number<T>(value: &Patch<T>) -> T
where
    T: Copy + Default,
{
    match value {
        Patch::Value(value) => *value,
        Patch::Missing => T::default(),
    }
}

fn optional_number<T>(value: &Patch<T>) -> Option<T>
where
    T: Copy,
{
    match value {
        Patch::Value(value) => Some(*value),
        Patch::Missing => None,
    }
}

fn list<T: Clone>(value: &Patch<Vec<T>>) -> Vec<T> {
    match value {
        Patch::Value(value) => value.clone(),
        Patch::Missing => Vec::new(),
    }
}

fn validation(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}
