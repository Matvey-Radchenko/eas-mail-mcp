use crate::wbxml::encode;
use crate::{CalendarApplication, EasError, MutationResult, Result};

use super::tree::{element, push_text};

/// Builds a Calendar Sync/Add command for one item, including supported recurrence and exceptions.
pub fn build_calendar_add(
    collection_id: &str,
    sync_key: &str,
    client_id: &str,
    item: &CalendarApplication,
) -> Result<Vec<u8>> {
    build_calendar_mutation(collection_id, sync_key, CalendarCommand::Add(client_id), Some(item))
}

/// Builds a Calendar Sync/Change command with a complete merged item.
pub fn build_calendar_change(
    collection_id: &str,
    sync_key: &str,
    server_id: &str,
    item: &CalendarApplication,
) -> Result<Vec<u8>> {
    build_calendar_mutation(collection_id, sync_key, CalendarCommand::Change(server_id), Some(item))
}

/// Builds a Calendar Sync/Delete command.
pub fn build_calendar_delete(
    collection_id: &str,
    sync_key: &str,
    server_id: &str,
) -> Result<Vec<u8>> {
    build_calendar_mutation(collection_id, sync_key, CalendarCommand::Delete(server_id), None)
}

/// Parses Add, Change, or Delete Sync responses.
pub fn parse_calendar_mutation_sync(data: &[u8]) -> Result<MutationResult> {
    super::calendar_mutation_response::parse_unbound(data)
}

enum CalendarCommand<'a> {
    Add(&'a str),
    Change(&'a str),
    Delete(&'a str),
}

fn build_calendar_mutation(
    collection_id: &str,
    sync_key: &str,
    command: CalendarCommand<'_>,
    item: Option<&CalendarApplication>,
) -> Result<Vec<u8>> {
    if collection_id.is_empty() || sync_key.is_empty() {
        return Err(EasError::InvalidConfiguration(
            "Calendar mutation requires collection and SyncKey".into(),
        ));
    }
    let (name, identifier_name, identifier, permanent_delete) = match command {
        CalendarCommand::Add(value) => ("Add", "ClientId", value, false),
        CalendarCommand::Change(value) => ("Change", "ServerId", value, false),
        CalendarCommand::Delete(value) => ("Delete", "ServerId", value, true),
    };
    if identifier.is_empty() {
        return Err(EasError::InvalidConfiguration("Calendar mutation identifier is empty".into()));
    }
    let mut root = element("AirSync", "Sync");
    let mut collections = element("AirSync", "Collections");
    let mut collection = element("AirSync", "Collection");
    push_text(&mut collection, "AirSync", "SyncKey", sync_key);
    push_text(&mut collection, "AirSync", "CollectionId", collection_id);
    if permanent_delete {
        push_text(&mut collection, "AirSync", "DeletesAsMoves", "0");
    }
    push_text(&mut collection, "AirSync", "GetChanges", "0");
    let mut commands = element("AirSync", "Commands");
    let mut mutation = element("AirSync", name);
    push_text(&mut mutation, "AirSync", identifier_name, identifier);
    if let Some(item) = item {
        mutation.push(application_data(item)?);
    }
    commands.push(mutation);
    collection.push(commands);
    collections.push(collection);
    root.push(collections);
    encode(&root)
}

fn application_data(item: &CalendarApplication) -> Result<crate::wbxml::Element> {
    let mut application = element("AirSync", "ApplicationData");
    push_text(&mut application, "Calendar", "TimeZone", &item.time_zone);
    push_text(&mut application, "Calendar", "AllDayEvent", if item.all_day { "1" } else { "0" });
    push_text(&mut application, "Calendar", "BusyStatus", item.busy_status.to_string());
    push_text(&mut application, "Calendar", "DtStamp", eas_datetime(item.dt_stamp));
    push_text(&mut application, "Calendar", "StartTime", eas_datetime(item.starts_at));
    push_text(&mut application, "Calendar", "EndTime", eas_datetime(item.ends_at));
    push_text(&mut application, "Calendar", "Location", &item.location);
    if let Some(reminder) = item.reminder_minutes {
        push_text(&mut application, "Calendar", "Reminder", reminder.to_string());
    }
    super::calendar_properties_write::append(&mut application, &item.properties)?;
    push_text(&mut application, "Calendar", "Subject", &item.subject);
    push_text(&mut application, "Calendar", "UID", &item.uid);
    push_text(&mut application, "Calendar", "MeetingStatus", item.meeting_status.to_string());
    push_text(
        &mut application,
        "Calendar",
        "ResponseRequested",
        if item.response_requested { "1" } else { "0" },
    );
    application.push(attendees(&item.attendees));
    let mut body = element("AirSyncBase", "Body");
    push_text(&mut body, "AirSyncBase", "Type", "1");
    push_text(&mut body, "AirSyncBase", "Data", &item.body);
    application.push(body);
    Ok(application)
}

pub(super) fn attendees(values: &[crate::CalendarAttendee]) -> crate::wbxml::Element {
    let mut container = element("Calendar", "Attendees");
    for value in values {
        let mut attendee = element("Calendar", "Attendee");
        push_text(&mut attendee, "Calendar", "Email", &value.email);
        push_text(&mut attendee, "Calendar", "Name", &value.name);
        push_text(&mut attendee, "Calendar", "AttendeeStatus", value.attendee_status.to_string());
        push_text(&mut attendee, "Calendar", "AttendeeType", value.attendee_type.to_string());
        container.push(attendee);
    }
    container
}

fn eas_datetime(value: chrono::DateTime<chrono::Utc>) -> String {
    value.format("%Y%m%dT%H%M%SZ").to_string()
}
