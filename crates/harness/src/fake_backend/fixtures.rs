use std::collections::BTreeMap;

use eas_mail_mcp::backend::BackendEvent;
use eas_mail_protocol::{
    CalendarApplication, CalendarAttendee, CalendarFields, CollectionKind, Folder, Patch,
};

pub(super) fn folders() -> Vec<Folder> {
    vec![
        Folder {
            server_id: "inbox".into(),
            parent_id: "0".into(),
            display_name: "Inbox".into(),
            folder_type: 2,
            kind: Some(CollectionKind::Mail),
        },
        Folder {
            server_id: "calendar".into(),
            parent_id: "0".into(),
            display_name: "Calendar".into(),
            folder_type: 8,
            kind: Some(CollectionKind::Calendar),
        },
    ]
}

pub(super) fn event(account_id: &str) -> BackendEvent {
    BackendEvent {
        occurrence_start: None,
        account_id: account_id.into(),
        long_id: "event-1".into(),
        collection_id: Some("calendar".into()),
        server_id: Some("event-1".into()),
        fields: CalendarFields {
            properties: None,
            subject: Patch::Value("Planning".into()),
            body: Patch::Value("<p>Agenda</p>".into()),
            body_truncated: Patch::Value(false),
            starts_at: Patch::Value(chrono::DateTime::from_timestamp(1_700_010_000, 0)),
            ends_at: Patch::Value(chrono::DateTime::from_timestamp(1_700_013_600, 0)),
            all_day: Patch::Value(false),
            location: Patch::Value("Room 1".into()),
            organizer: Patch::Value("owner@example.invalid".into()),
            organizer_email: Patch::Value(format!("{account_id}@example.invalid")),
            attendees: Patch::Value(vec![CalendarAttendee {
                email: "guest@example.invalid".into(),
                name: "Guest".into(),
                attendee_type: 1,
                attendee_status: 0,
            }]),
            reminder_minutes: Patch::Value(15),
            recurrence: Patch::Value(BTreeMap::new()),
            exceptions: Patch::Value(Vec::new()),
            meeting_status: Patch::Value(1),
            uid: Patch::Value("event-uid@example.invalid".into()),
            dt_stamp: Patch::Value(chrono::DateTime::from_timestamp(1_700_000_000, 0)),
            time_zone: Patch::Value(format!("{}==", "A".repeat(230))),
            busy_status: Patch::Value(2),
            response_requested: Patch::Value(true),
            response_type: Patch::Value(5),
        },
    }
}

pub(super) fn event_from_application(account_id: &str, item: &CalendarApplication) -> BackendEvent {
    BackendEvent {
        occurrence_start: None,
        account_id: account_id.into(),
        long_id: String::new(),
        collection_id: Some("calendar".into()),
        server_id: Some("event-created".into()),
        fields: CalendarFields {
            properties: Some(item.properties.clone()),
            subject: Patch::Value(item.subject.clone()),
            body: Patch::Value(item.body.clone()),
            body_truncated: Patch::Value(false),
            starts_at: Patch::Value(Some(item.starts_at)),
            ends_at: Patch::Value(Some(item.ends_at)),
            all_day: Patch::Value(item.all_day),
            location: Patch::Value(item.location.clone()),
            organizer_email: Patch::Value(format!("{account_id}@example.invalid")),
            attendees: Patch::Value(item.attendees.clone()),
            reminder_minutes: item.reminder_minutes.map_or(Patch::Missing, Patch::Value),
            recurrence: Patch::Value(
                item.properties
                    .recurrence
                    .as_ref()
                    .map(eas_mail_protocol::CalendarRecurrence::to_fields)
                    .unwrap_or_default(),
            ),
            exceptions: Patch::Value(
                item.properties
                    .exceptions
                    .iter()
                    .map(eas_mail_protocol::protocol::exception_fields)
                    .collect(),
            ),
            meeting_status: Patch::Value(item.meeting_status),
            uid: Patch::Value(item.uid.clone()),
            dt_stamp: Patch::Value(Some(item.dt_stamp)),
            time_zone: Patch::Value(item.time_zone.clone()),
            busy_status: Patch::Value(item.busy_status),
            response_requested: Patch::Value(item.response_requested),
            ..CalendarFields::default()
        },
    }
}

pub(super) fn received_event(account_id: &str) -> BackendEvent {
    let mut value = event(account_id);
    value.long_id = "received-event".into();
    value.server_id = Some("received-event".into());
    value.fields.organizer = Patch::Value("External Organizer".into());
    value.fields.organizer_email = Patch::Value("organizer@example.invalid".into());
    value.fields.meeting_status = Patch::Value(3);
    value
}

pub(super) fn personal_event(account_id: &str) -> BackendEvent {
    let mut value = event(account_id);
    value.long_id = "personal-event".into();
    value.server_id = Some("personal-event".into());
    value.fields.attendees = Patch::Value(Vec::new());
    value.fields.meeting_status = Patch::Value(0);
    value.fields.response_requested = Patch::Value(false);
    value
}

pub(super) fn recurring_event(account_id: &str) -> BackendEvent {
    let mut value = event(account_id);
    value.long_id = "recurring-event".into();
    value.server_id = Some("recurring-event".into());
    let rule = eas_mail_protocol::CalendarRecurrence {
        pattern: eas_mail_protocol::RecurrencePattern::Daily,
        interval: 1,
        first_day_of_week: 1,
        end: eas_mail_protocol::RecurrenceEnd::Count(3),
    };
    value.fields.recurrence = Patch::Value(rule.to_fields());
    value.fields.properties = Some(eas_mail_protocol::CalendarProperties {
        recurrence: Some(rule),
        ..Default::default()
    });
    value
}
