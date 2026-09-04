use eas_mail_protocol::Patch;

use crate::backend::{BackendEvent, BackendMail};
use crate::model::{
    CalendarAttendeeRole, CalendarAttendeeStatus, CalendarAttendeeView, CalendarBusyStatus,
    CalendarEvent, CalendarEventSummary, CalendarEventType, CalendarMailKind, MailDetail,
    MailSummary,
};
use crate::sanitize::{plain_text, truncate};
use crate::{Result, Runtime};

impl Runtime {
    pub(super) fn mail_summary(&self, mail: BackendMail) -> Result<MailSummary> {
        let mail_ref = self.references.insert_mail(mail.clone())?;
        Ok(mail_summary(mail_ref, &mail))
    }

    pub(super) fn mail_detail(
        &self,
        mail_ref: String,
        mail: &BackendMail,
        requested_limit: usize,
    ) -> MailDetail {
        let mut summary = mail_summary(mail_ref, mail);
        let body = plain_text(string(&mail.fields.body));
        let (body, application_truncated) = truncate(&body, requested_limit);
        summary.preview = truncate(&body, 500).0;
        MailDetail {
            summary,
            cc: string(&mail.fields.cc).to_owned(),
            body,
            body_truncated: boolean(&mail.fields.body_truncated) || application_truncated,
        }
    }
}

pub(super) fn calendar_event(
    event_ref: String,
    event: &BackendEvent,
    account_email: &str,
    requested_limit: usize,
) -> CalendarEvent {
    let fields = &event.fields;
    let body = plain_text(string(&fields.body));
    let (body, application_truncated) = truncate(&body, requested_limit);
    let attendees = list(&fields.attendees)
        .into_iter()
        .map(|value| CalendarAttendeeView {
            email: plain_text(&value.email),
            name: plain_text(&value.name),
            role: attendee_role(value.attendee_type),
            status: attendee_status(value.attendee_status),
            untrusted_external_content: true,
        })
        .collect::<Vec<_>>();
    let writable = super::calendar_series::properties(event).is_ok();
    let organizer_email = string(&fields.organizer_email);
    let event_type = event_type(event, account_email);
    let can_update = writable
        && matches!(event_type, CalendarEventType::Personal | CalendarEventType::OrganizerMeeting);
    CalendarEvent {
        event_ref,
        account_id: event.account_id.clone(),
        subject: plain_text(string(&fields.subject)),
        body,
        body_truncated: boolean(&fields.body_truncated) || application_truncated,
        starts_at: optional_datetime_string(&fields.starts_at),
        ends_at: optional_datetime_string(&fields.ends_at),
        all_day: boolean(&fields.all_day),
        location: plain_text(string(&fields.location)),
        organizer: plain_text(string(&fields.organizer)),
        organizer_email: plain_text(organizer_email),
        uid: plain_text(string(&fields.uid)),
        event_type,
        busy_status: busy_status(number(&fields.busy_status)),
        response_status: attendee_status(number(&fields.response_type)),
        attendees,
        recurrence: map(&fields.recurrence),
        exceptions: nested_map(&fields.exceptions),
        can_update,
        can_delete: can_update && event_type == CalendarEventType::Personal,
        can_cancel: can_update && event_type == CalendarEventType::OrganizerMeeting,
        can_respond: writable && event_type == CalendarEventType::AttendeeMeeting,
        untrusted_external_content: true,
    }
}

fn event_type(event: &BackendEvent, account_email: &str) -> CalendarEventType {
    use super::calendar_prepare::EventOwnership;
    match super::calendar_prepare::ownership(event, account_email) {
        EventOwnership::Personal => CalendarEventType::Personal,
        EventOwnership::Organizer => CalendarEventType::OrganizerMeeting,
        EventOwnership::Attendee => CalendarEventType::AttendeeMeeting,
    }
}

fn attendee_role(value: u8) -> CalendarAttendeeRole {
    match value {
        2 => CalendarAttendeeRole::Optional,
        3 => CalendarAttendeeRole::Resource,
        _ => CalendarAttendeeRole::Required,
    }
}

fn attendee_status(value: u8) -> CalendarAttendeeStatus {
    match value {
        2 => CalendarAttendeeStatus::Tentative,
        3 => CalendarAttendeeStatus::Accepted,
        4 => CalendarAttendeeStatus::Declined,
        0 | 5 => CalendarAttendeeStatus::NoResponse,
        _ => CalendarAttendeeStatus::Unknown,
    }
}

fn busy_status(value: u8) -> CalendarBusyStatus {
    match value {
        0 => CalendarBusyStatus::Free,
        1 => CalendarBusyStatus::Tentative,
        3 => CalendarBusyStatus::OutOfOffice,
        _ => CalendarBusyStatus::Busy,
    }
}

pub(super) fn calendar_event_summary(
    event_ref: String,
    event: &BackendEvent,
) -> CalendarEventSummary {
    let fields = &event.fields;
    CalendarEventSummary {
        event_ref,
        account_id: event.account_id.clone(),
        subject: plain_text(string(&fields.subject)),
        starts_at: optional_datetime_string(&fields.starts_at),
        ends_at: optional_datetime_string(&fields.ends_at),
        all_day: boolean(&fields.all_day),
        location: plain_text(string(&fields.location)),
        organizer: plain_text(string(&fields.organizer)),
        attendee_count: u32::try_from(list(&fields.attendees).len()).unwrap_or(u32::MAX),
        recurring: !map(&fields.recurrence).is_empty()
            || !nested_map(&fields.exceptions).is_empty(),
        untrusted_external_content: true,
    }
}

fn mail_summary(mail_ref: String, mail: &BackendMail) -> MailSummary {
    let preview = truncate(&plain_text(string(&mail.fields.body)), 500).0;
    let (calendar_message, can_respond) = calendar_mail(&mail.fields);
    MailSummary {
        mail_ref,
        account_id: mail.account_id.clone(),
        folder_id: mail.folder_id.clone(),
        subject: string(&mail.fields.subject).to_owned(),
        sender: string(&mail.fields.sender).to_owned(),
        recipients: string(&mail.fields.recipients).to_owned(),
        received_at: optional_datetime(&mail.fields.received_at),
        preview,
        is_read: boolean(&mail.fields.is_read),
        has_attachments: !list(&mail.fields.attachments).is_empty(),
        flag: match &mail.fields.flag {
            Patch::Value(flag) => match flag
                .child("Email", "Status")
                .map(eas_mail_protocol::wbxml::Element::text_content)
                .as_deref()
            {
                Some("0") | None if flag.children().next().is_none() => {
                    Some(crate::MailFlagState::None)
                }
                Some("0") => Some(crate::MailFlagState::None),
                Some("1") => Some(crate::MailFlagState::Complete),
                Some("2") => Some(crate::MailFlagState::Active),
                _ => None,
            },
            Patch::Missing => None,
        },
        categories: match &mail.fields.categories {
            Patch::Value(value) => Some(value.clone()),
            Patch::Missing => None,
        },
        calendar_message,
        can_respond,
        untrusted_external_content: true,
    }
}

fn calendar_mail(fields: &eas_mail_protocol::MailFields) -> (Option<CalendarMailKind>, bool) {
    let class = string(&fields.message_class).to_ascii_lowercase();
    let meeting = match &fields.meeting_request {
        Patch::Value(value) => Some(value),
        Patch::Missing => None,
    };
    if class.contains(".meeting.request") {
        let message_type = meeting.map_or(0, |value| value.message_type);
        let kind = match message_type {
            1 => CalendarMailKind::Request,
            2 | 3 => CalendarMailKind::Update,
            _ => CalendarMailKind::Other,
        };
        let can_respond = meeting.is_some_and(|value| {
            matches!(value.message_type, 1 | 2)
                && value.instance_type == 0
                && value.response_requested
        });
        (Some(kind), can_respond)
    } else if class.contains(".meeting.canceled") {
        (Some(CalendarMailKind::Cancellation), false)
    } else if class.contains(".meeting.resp") {
        (Some(CalendarMailKind::Response), false)
    } else if meeting.is_some() {
        (Some(CalendarMailKind::Other), false)
    } else {
        (None, false)
    }
}

pub(super) fn string(value: &Patch<String>) -> &str {
    match value {
        Patch::Value(value) => value,
        Patch::Missing => "",
    }
}

pub(super) fn boolean(value: &Patch<bool>) -> bool {
    matches!(value, Patch::Value(true))
}

pub(super) fn number<T>(value: &Patch<T>) -> T
where
    T: Copy + Default,
{
    match value {
        Patch::Value(value) => *value,
        Patch::Missing => T::default(),
    }
}

pub(super) fn list<T: Clone>(value: &Patch<Vec<T>>) -> Vec<T> {
    match value {
        Patch::Value(value) => value.clone(),
        Patch::Missing => Vec::new(),
    }
}

pub(super) fn folder_role(folder_type: u16) -> &'static str {
    match folder_type {
        2 => "inbox",
        3 => "drafts",
        4 => "trash",
        5 => "sent",
        6 => "outbox",
        8 => "calendar",
        12 => "user_mail",
        13 => "user_calendar",
        _ => "other",
    }
}

fn map(
    value: &Patch<std::collections::BTreeMap<String, String>>,
) -> std::collections::BTreeMap<String, String> {
    match value {
        Patch::Value(value) => value.clone(),
        Patch::Missing => std::collections::BTreeMap::new(),
    }
}

fn nested_map(
    value: &Patch<Vec<std::collections::BTreeMap<String, String>>>,
) -> Vec<std::collections::BTreeMap<String, String>> {
    list(value)
}

fn optional_datetime(
    value: &Patch<Option<chrono::DateTime<chrono::Utc>>>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    match value {
        Patch::Value(value) => *value,
        Patch::Missing => None,
    }
}

fn optional_datetime_string(
    value: &Patch<Option<chrono::DateTime<chrono::Utc>>>,
) -> Option<String> {
    optional_datetime(value).map(|item| item.to_rfc3339())
}
