use chrono::NaiveDate;
use eas_mail_protocol::{CalendarApplication, CalendarAttendee};
use icalendar::{
    Attendee, CUType, Calendar, Component as _, Event, EventLike as _, PartStat, Property, Role,
};
use mail_builder::MessageBuilder;
use mail_builder::encoders::base64::base64_encode_mime;
use mail_builder::headers::address::Address;
use mail_builder::headers::content_type::ContentType;
use mail_builder::headers::raw::Raw;
use mail_builder::headers::text::Text;
use mail_builder::mime::MimePart;

use crate::model::CalendarResponseChoice;
use crate::{AppError, ErrorCode, Result};

#[derive(Clone, Copy)]
pub(super) enum CalendarMessageMethod {
    Request,
    Cancel,
    Reply(CalendarResponseChoice),
}

pub(super) fn build(
    sender: &str,
    recipients: &[CalendarAttendee],
    item: &CalendarApplication,
    all_day_dates: Option<(NaiveDate, NaiveDate)>,
    method: CalendarMessageMethod,
    comment: &str,
) -> Result<Vec<u8>> {
    validate_mailbox(sender)?;
    if recipients.is_empty() || recipients.len() > 100 {
        return Err(validation("calendar notification requires 1-100 recipients"));
    }
    for recipient in recipients {
        validate_mailbox(&recipient.email)?;
        validate_header_text(&recipient.name)?;
    }
    validate_header_text(&item.subject)?;
    let method_name = method_name(method);
    let calendar = calendar(sender, recipients, item, all_day_dates, method)?;
    let mut encoded_calendar = Vec::new();
    base64_encode_mime(calendar.as_bytes(), &mut encoded_calendar, false).map_err(|_| {
        AppError::new(ErrorCode::ProtocolError, "cannot encode calendar MIME message")
    })?;
    let calendar_part = MimePart::new(
        ContentType::new("text/calendar")
            .attribute("charset", "utf-8")
            .attribute("method", method_name),
        encoded_calendar,
    )
    .transfer_encoding("base64");
    let plain_body = if comment.is_empty() { item.body.clone() } else { comment.to_owned() };
    let body = MimePart::new(
        "multipart/alternative",
        vec![MimePart::new("text/plain", plain_body), calendar_part],
    );
    let addresses = recipients
        .iter()
        .map(|value| {
            Address::new_address(
                (!value.name.is_empty()).then_some(value.name.clone()),
                value.email.clone(),
            )
        })
        .collect::<Vec<_>>();
    MessageBuilder::new()
        .from(sender.to_owned())
        .to(Address::new_list(addresses))
        .subject(message_subject(method, &item.subject))
        .header("Thread-Topic", Text::new(item.subject.clone()))
        .header("Content-Class", Raw::new("urn:content-classes:calendarmessage"))
        .body(body)
        .write_to_vec()
        .map_err(|_| AppError::new(ErrorCode::ProtocolError, "cannot build calendar MIME message"))
}

fn calendar(
    sender: &str,
    recipients: &[CalendarAttendee],
    item: &CalendarApplication,
    all_day_dates: Option<(NaiveDate, NaiveDate)>,
    method: CalendarMessageMethod,
) -> Result<String> {
    let mut calendar = Calendar::empty();
    calendar
        .append_property(Property::new("PRODID", "-//EAS Mail MCP//EN"))
        .append_property(Property::new("VERSION", "2.0"))
        .append_property(Property::new("CALSCALE", "GREGORIAN"))
        .append_property(Property::new("METHOD", method_name(method)));
    let attendees = notification_attendees(item, recipients, method);
    if !matches!(method, CalendarMessageMethod::Cancel) || !attendees.is_empty() {
        calendar.push(event(sender, &attendees, item, all_day_dates, method)?);
    }
    super::calendar_mime_series::timezone(&mut calendar, item)?;
    for exception in item.properties.exceptions.iter().filter(|value| !value.deleted) {
        let occurrence = super::calendar_series::selected(item, exception.original_start)?;
        let prepared = super::calendar_series::prepared(occurrence)?;
        let attendees = notification_attendees(&prepared.mutation.application, recipients, method);
        if matches!(method, CalendarMessageMethod::Cancel) && attendees.is_empty() {
            continue;
        }
        calendar.push(event(
            sender,
            &attendees,
            &prepared.mutation.application,
            prepared.all_day_dates,
            method,
        )?);
    }
    Ok(calendar.to_string())
}

fn notification_attendees(
    item: &CalendarApplication,
    recipients: &[CalendarAttendee],
    method: CalendarMessageMethod,
) -> Vec<CalendarAttendee> {
    match method {
        CalendarMessageMethod::Reply(_) => recipients.to_vec(),
        CalendarMessageMethod::Request => item.attendees.clone(),
        CalendarMessageMethod::Cancel => item
            .attendees
            .iter()
            .filter(|attendee| {
                recipients
                    .iter()
                    .any(|recipient| recipient.email.eq_ignore_ascii_case(&attendee.email))
            })
            .cloned()
            .collect(),
    }
}

fn event(
    sender: &str,
    recipients: &[CalendarAttendee],
    item: &CalendarApplication,
    all_day_dates: Option<(NaiveDate, NaiveDate)>,
    method: CalendarMessageMethod,
) -> Result<Event> {
    let mut event = Event::new();
    event
        .uid(&item.uid)
        .timestamp(item.dt_stamp)
        .summary(&item.subject)
        .description(&item.body)
        .location(&item.location);
    super::calendar_mime_series::schedule(&mut event, item, all_day_dates)?;
    let organizer = match method {
        CalendarMessageMethod::Reply(_) => recipients
            .first()
            .map(|value| value.email.as_str())
            .ok_or_else(|| validation("calendar reply has no organizer"))?,
        CalendarMessageMethod::Request | CalendarMessageMethod::Cancel => sender,
    };
    event.append_property(Property::new("ORGANIZER", format!("mailto:{organizer}")));
    match method {
        CalendarMessageMethod::Reply(response) => {
            event.attendee(
                Attendee::new(format!("mailto:{sender}"))
                    .role(Role::ReqParticipant)
                    .partstat(part_stat(response))
                    .rsvp(false),
            );
        }
        CalendarMessageMethod::Request | CalendarMessageMethod::Cancel => {
            for attendee in recipients {
                let mut value = Attendee::new(format!("mailto:{}", attendee.email))
                    .role(role(attendee.attendee_type))
                    .partstat(PartStat::NeedsAction)
                    .rsvp(true);
                if attendee.attendee_type == 3 {
                    value = value.cutype(CUType::Resource);
                }
                if !attendee.name.is_empty() {
                    value = value.cn(attendee.name.clone());
                }
                event.attendee(value);
            }
        }
    }
    event.status(if matches!(method, CalendarMessageMethod::Cancel) {
        icalendar::EventStatus::Cancelled
    } else {
        icalendar::EventStatus::Confirmed
    });
    Ok(event)
}

const fn method_name(method: CalendarMessageMethod) -> &'static str {
    match method {
        CalendarMessageMethod::Request => "REQUEST",
        CalendarMessageMethod::Cancel => "CANCEL",
        CalendarMessageMethod::Reply(_) => "REPLY",
    }
}

fn message_subject(method: CalendarMessageMethod, subject: &str) -> String {
    match method {
        CalendarMessageMethod::Request => subject.to_owned(),
        CalendarMessageMethod::Cancel => format!("Cancelled: {subject}"),
        CalendarMessageMethod::Reply(CalendarResponseChoice::Accept) => {
            format!("Accepted: {subject}")
        }
        CalendarMessageMethod::Reply(CalendarResponseChoice::Tentative) => {
            format!("Tentative: {subject}")
        }
        CalendarMessageMethod::Reply(CalendarResponseChoice::Decline) => {
            format!("Declined: {subject}")
        }
    }
}

const fn role(value: u8) -> Role {
    match value {
        2 => Role::OptParticipant,
        3 => Role::NonParticipant,
        _ => Role::ReqParticipant,
    }
}

const fn part_stat(value: CalendarResponseChoice) -> PartStat {
    match value {
        CalendarResponseChoice::Accept => PartStat::Accepted,
        CalendarResponseChoice::Tentative => PartStat::Tentative,
        CalendarResponseChoice::Decline => PartStat::Declined,
    }
}

fn validate_mailbox(value: &str) -> Result<()> {
    let trimmed = value.trim();
    let valid = trimmed == value
        && !value.chars().any(char::is_control)
        && value.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        });
    if valid { Ok(()) } else { Err(validation("calendar attendee email is invalid")) }
}

fn validate_header_text(value: &str) -> Result<()> {
    if value.chars().any(|value| matches!(value, '\r' | '\n' | '\0')) {
        Err(validation("calendar header text contains a control character"))
    } else {
        Ok(())
    }
}

fn validation(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}

#[cfg(test)]
#[path = "calendar_mime/series_tests.rs"]
mod series_tests;
#[cfg(test)]
mod tests;
