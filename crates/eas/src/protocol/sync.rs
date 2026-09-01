use std::collections::BTreeMap;

use crate::wbxml::{Element, decode, encode};
use crate::{
    Attachment, CalendarFields, ChangeData, ChangeKind, CollectionKind, EasError, MailFields,
    MeetingRequest, MutationResult, Patch, Result, SyncChange, SyncPage,
};

use super::tree::{direct_text, element, integer, parse_datetime, push_text};

/// Builds a metadata-only collection Sync request.
pub fn build_sync(
    collection_id: &str,
    sync_key: &str,
    kind: CollectionKind,
    filter_type: u8,
    preview_size: usize,
) -> Result<Vec<u8>> {
    let mut root = element("AirSync", "Sync");
    let mut collections = element("AirSync", "Collections");
    let mut collection = element("AirSync", "Collection");
    push_text(&mut collection, "AirSync", "SyncKey", sync_key);
    push_text(&mut collection, "AirSync", "CollectionId", collection_id);
    if sync_key != "0" {
        if kind == CollectionKind::Mail {
            push_text(&mut collection, "AirSync", "DeletesAsMoves", "1");
        }
        push_text(&mut collection, "AirSync", "GetChanges", "1");
        push_text(&mut collection, "AirSync", "WindowSize", "100");
        let mut options = element("AirSync", "Options");
        push_text(&mut options, "AirSync", "FilterType", filter_type.to_string());
        push_text(&mut options, "AirSync", "Conflict", "1");
        let mut preference = element("AirSyncBase", "BodyPreference");
        push_text(&mut preference, "AirSyncBase", "Type", "1");
        push_text(
            &mut preference,
            "AirSyncBase",
            "TruncationSize",
            preview_size.min(500).to_string(),
        );
        push_text(&mut preference, "AirSyncBase", "AllOrNone", "0");
        options.push(preference);
        collection.push(options);
    }
    collections.push(collection);
    root.push(collections);
    encode(&root)
}

/// Parses one ordered Sync page, preserving absent and empty fields.
pub fn parse_sync(data: &[u8], kind: CollectionKind) -> Result<SyncPage> {
    let root = decode(data)?
        .ok_or_else(|| EasError::Protocol("Exchange returned an empty Sync response".into()))?;
    let account_status = root
        .child("AirSync", "Status")
        .map(|value| integer(Some(value.text_content()), 1))
        .unwrap_or(1);
    let collection = root
        .descendant("AirSync", "Collection")
        .ok_or_else(|| EasError::Protocol("Sync response has no collection".into()))?;
    let collection_status = integer(direct_text(collection, "AirSync", "Status"), 1);
    let sync_key = direct_text(collection, "AirSync", "SyncKey").unwrap_or_default();
    let more_available = collection.child("AirSync", "MoreAvailable").is_some();
    let mut changes = Vec::new();
    if let Some(commands) = collection.child("AirSync", "Commands") {
        for command in commands.children() {
            let change_kind = match command.name.as_str() {
                "Add" => ChangeKind::Add,
                "Change" => ChangeKind::Change,
                "Delete" => ChangeKind::Delete,
                "SoftDelete" => ChangeKind::SoftDelete,
                _ => continue,
            };
            let server_id = direct_text(command, "AirSync", "ServerId").unwrap_or_default();
            if server_id.is_empty() {
                continue;
            }
            let data = command.child("AirSync", "ApplicationData").map_or(
                ChangeData::None,
                |application| match kind {
                    CollectionKind::Mail => ChangeData::Mail(parse_mail_fields(application)),
                    CollectionKind::Calendar => {
                        ChangeData::Calendar(parse_calendar_fields(application))
                    }
                },
            );
            changes.push(SyncChange { kind: change_kind, server_id, data });
        }
    }
    Ok(SyncPage { account_status, collection_status, sync_key, more_available, changes })
}

/// Builds a read-state mutation using the current collection SyncKey.
pub fn build_mark_read(
    collection_id: &str,
    server_id: &str,
    sync_key: &str,
    is_read: bool,
) -> Result<Vec<u8>> {
    let mut root = element("AirSync", "Sync");
    let mut collections = element("AirSync", "Collections");
    let mut collection = element("AirSync", "Collection");
    push_text(&mut collection, "AirSync", "SyncKey", sync_key);
    push_text(&mut collection, "AirSync", "CollectionId", collection_id);
    push_text(&mut collection, "AirSync", "GetChanges", "0");
    let mut commands = element("AirSync", "Commands");
    let mut change = element("AirSync", "Change");
    push_text(&mut change, "AirSync", "ServerId", server_id);
    let mut application = element("AirSync", "ApplicationData");
    push_text(&mut application, "Email", "Read", if is_read { "1" } else { "0" });
    change.push(application);
    commands.push(change);
    collection.push(commands);
    collections.push(collection);
    root.push(collections);
    encode(&root)
}

/// Parses a Sync mutation response.
pub fn parse_mutation_sync(data: &[u8]) -> Result<MutationResult> {
    let page = parse_sync(data, CollectionKind::Mail)?;
    let root = decode(data)?
        .ok_or_else(|| EasError::Protocol("Exchange returned an empty Sync response".into()))?;
    let status = root
        .descendant("AirSync", "Responses")
        .and_then(|responses| responses.children().next())
        .and_then(|response| direct_text(response, "AirSync", "Status"))
        .map_or(1, |value| integer(Some(value), 1));
    Ok(MutationResult { status, sync_key: Some(page.sync_key), server_id: None })
}

pub(super) fn parse_mail_fields(application: &Element) -> MailFields {
    let (body, body_truncated) =
        application.child("AirSyncBase", "Body").map_or((Patch::Missing, Patch::Missing), |body| {
            let content =
                Patch::Value(direct_text(body, "AirSyncBase", "Data").unwrap_or_default());
            let truncated = body
                .child("AirSyncBase", "Truncated")
                .map_or(Patch::Value(false), |value| Patch::Value(value.text_content() == "1"));
            (content, truncated)
        });
    MailFields {
        subject: string_patch(application, "Email", "Subject"),
        sender: string_patch(application, "Email", "From"),
        recipients: string_patch(application, "Email", "To"),
        cc: string_patch(application, "Email", "Cc"),
        received_at: application.child("Email", "DateReceived").map_or(Patch::Missing, |value| {
            Patch::Value(parse_datetime(Some(value.text_content())))
        }),
        body,
        body_truncated,
        is_read: bool_patch(application, "Email", "Read"),
        importance: application.child("Email", "Importance").map_or(Patch::Missing, |value| {
            Patch::Value(value.text_content().parse().unwrap_or(1))
        }),
        attachments: application
            .child("AirSyncBase", "Attachments")
            .map_or(Patch::Missing, |container| Patch::Value(parse_attachments(container))),
        message_class: string_patch(application, "Email", "MessageClass"),
        meeting_request: application
            .child("Email", "MeetingRequest")
            .map_or(Patch::Missing, |value| Patch::Value(parse_meeting_request(value))),
    }
}

fn parse_meeting_request(value: &Element) -> MeetingRequest {
    MeetingRequest {
        all_day: direct_text(value, "Email", "AllDayEvent").is_some_and(|value| value == "1"),
        dt_stamp: parse_datetime(direct_text(value, "Email", "DtStamp")),
        starts_at: parse_datetime(direct_text(value, "Email", "StartTime")),
        ends_at: parse_datetime(direct_text(value, "Email", "EndTime")),
        instance_type: direct_text(value, "Email", "InstanceType")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
        location: direct_text(value, "Email", "Location").unwrap_or_default(),
        organizer: direct_text(value, "Email", "Organizer").unwrap_or_default(),
        reminder_minutes: direct_text(value, "Email", "Reminder")
            .and_then(|value| value.parse().ok()),
        response_requested: direct_text(value, "Email", "ResponseRequested")
            .is_some_and(|value| value == "1"),
        busy_status: direct_text(value, "Email", "BusyStatus")
            .and_then(|value| value.parse().ok())
            .unwrap_or(2),
        time_zone: direct_text(value, "Email", "TimeZone").unwrap_or_default(),
        global_object_id: direct_text(value, "Email", "GlobalObjId").unwrap_or_default(),
        uid: direct_text(value, "Calendar", "UID").unwrap_or_default(),
        message_type: direct_text(value, "Email2", "MeetingMessageType")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
    }
}

pub(super) fn parse_calendar_fields(application: &Element) -> CalendarFields {
    let mut fields = parse_calendar_fields_base(application);
    fields.properties = Some(super::calendar_properties::parse(application));
    fields
}

pub(super) fn parse_calendar_fields_base(application: &Element) -> CalendarFields {
    let organizer = match string_patch(application, "Calendar", "OrganizerName") {
        Patch::Missing => string_patch(application, "Calendar", "OrganizerEmail"),
        value => value,
    };
    CalendarFields {
        properties: None,
        subject: string_patch(application, "Calendar", "Subject"),
        body: application.child("AirSyncBase", "Body").map_or(Patch::Missing, |body| {
            Patch::Value(direct_text(body, "AirSyncBase", "Data").unwrap_or_default())
        }),
        body_truncated: application
            .child("AirSyncBase", "Body")
            .and_then(|body| body.child("AirSyncBase", "Truncated"))
            .map_or(Patch::Missing, |value| Patch::Value(value.text_content() == "1")),
        starts_at: datetime_patch(application, "StartTime"),
        ends_at: datetime_patch(application, "EndTime"),
        all_day: bool_patch(application, "Calendar", "AllDayEvent"),
        location: string_patch(application, "Calendar", "Location"),
        organizer,
        organizer_email: string_patch(application, "Calendar", "OrganizerEmail"),
        attendees: application.child("Calendar", "Attendees").map_or(Patch::Missing, |container| {
            Patch::Value(
                container
                    .children()
                    .filter(|child| child.name == "Attendee")
                    .filter_map(|attendee| {
                        let email = direct_text(attendee, "Calendar", "Email")?;
                        (!email.is_empty()).then(|| crate::CalendarAttendee {
                            email,
                            name: direct_text(attendee, "Calendar", "Name").unwrap_or_default(),
                            attendee_type: direct_text(attendee, "Calendar", "AttendeeType")
                                .and_then(|value| value.parse().ok())
                                .unwrap_or(1),
                            attendee_status: direct_text(attendee, "Calendar", "AttendeeStatus")
                                .and_then(|value| value.parse().ok())
                                .unwrap_or(0),
                        })
                    })
                    .collect(),
            )
        }),
        reminder_minutes: number_patch(application, "Reminder"),
        recurrence: recurrence_patch(application),
        exceptions: exceptions_patch(application),
        meeting_status: number_patch(application, "MeetingStatus"),
        uid: string_patch(application, "Calendar", "UID"),
        dt_stamp: datetime_patch(application, "DtStamp"),
        time_zone: string_patch(application, "Calendar", "TimeZone"),
        busy_status: number_patch(application, "BusyStatus"),
        response_requested: bool_patch(application, "Calendar", "ResponseRequested"),
        response_type: number_patch(application, "ResponseType"),
    }
}

fn parse_attachments(container: &Element) -> Vec<Attachment> {
    container
        .children()
        .filter(|child| child.name == "Attachment")
        .filter_map(|item| {
            let file_reference = direct_text(item, "AirSyncBase", "FileReference")?;
            (!file_reference.is_empty()).then(|| Attachment {
                display_name: safe_filename(
                    &direct_text(item, "AirSyncBase", "DisplayName")
                        .unwrap_or_else(|| "attachment".into()),
                ),
                file_reference,
                size: direct_text(item, "AirSyncBase", "EstimatedDataSize")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0),
                content_type: direct_text(item, "AirSyncBase", "ContentType")
                    .unwrap_or_else(|| "application/octet-stream".into()),
                is_inline: direct_text(item, "AirSyncBase", "IsInline").as_deref() == Some("1"),
                content_id: direct_text(item, "AirSyncBase", "ContentId").unwrap_or_default(),
            })
        })
        .collect()
}

fn string_patch(parent: &Element, namespace: &str, name: &str) -> Patch<String> {
    parent.child(namespace, name).map_or(Patch::Missing, |value| Patch::Value(value.text_content()))
}

fn bool_patch(parent: &Element, namespace: &str, name: &str) -> Patch<bool> {
    parent
        .child(namespace, name)
        .map_or(Patch::Missing, |value| Patch::Value(value.text_content() == "1"))
}

fn number_patch<T>(parent: &Element, name: &str) -> Patch<T>
where
    T: std::str::FromStr + Default,
{
    parent.child("Calendar", name).map_or(Patch::Missing, |value| {
        Patch::Value(value.text_content().parse().unwrap_or_default())
    })
}

fn datetime_patch(parent: &Element, name: &str) -> Patch<Option<chrono::DateTime<chrono::Utc>>> {
    parent
        .child("Calendar", name)
        .map_or(Patch::Missing, |value| Patch::Value(parse_datetime(Some(value.text_content()))))
}

fn recurrence_patch(parent: &Element) -> Patch<BTreeMap<String, String>> {
    parent.child("Calendar", "Recurrence").map_or(Patch::Missing, |recurrence| {
        Patch::Value(
            recurrence
                .children()
                .map(|value| (value.name.to_ascii_lowercase(), value.text_content()))
                .collect(),
        )
    })
}

fn exceptions_patch(parent: &Element) -> Patch<Vec<BTreeMap<String, String>>> {
    parent.child("Calendar", "Exceptions").map_or(Patch::Missing, |exceptions| {
        Patch::Value(
            exceptions
                .children()
                .filter(|value| value.name == "Exception")
                .map(|exception| {
                    exception
                        .children()
                        .map(|value| (value.name.to_ascii_lowercase(), value.text_content()))
                        .collect()
                })
                .collect(),
        )
    })
}

fn safe_filename(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '\0' => '_',
            other if other.is_control() => '_',
            other => other,
        })
        .collect::<String>();
    let output = sanitized.trim_matches(['.', ' ']);
    if output.is_empty() { "attachment".into() } else { output.chars().take(255).collect() }
}
