use chrono::{TimeZone as _, Utc};
use eas_mail_protocol::protocol::{
    build_calendar_add, build_calendar_change, build_calendar_delete, build_meeting_response,
    build_meeting_response_long_id, parse_calendar_mutation_sync, parse_meeting_response,
};
use eas_mail_protocol::wbxml::{Element, decode, encode};
use eas_mail_protocol::{CalendarApplication, CalendarAttendee, EasError, MeetingResponseChoice};

#[test]
fn calendar_add_and_change_emit_complete_non_recurring_items() -> anyhow::Result<()> {
    let item = calendar_item()?;
    for (data, command, identifier, expected) in [
        (build_calendar_add("calendar", "41", "client-1", &item)?, "Add", "ClientId", "client-1"),
        (
            build_calendar_change("calendar", "41", "server-1", &item)?,
            "Change",
            "ServerId",
            "server-1",
        ),
    ] {
        let root = required_root(&data)?;
        assert_eq!(text(&root, "AirSync", "SyncKey").as_deref(), Some("41"));
        assert_eq!(text(&root, "AirSync", "CollectionId").as_deref(), Some("calendar"));
        let mutation = root
            .descendant("AirSync", command)
            .ok_or_else(|| anyhow::anyhow!("calendar mutation is missing"))?;
        assert_eq!(text(mutation, "AirSync", identifier).as_deref(), Some(expected));
        assert_eq!(text(mutation, "Calendar", "UID").as_deref(), Some("uid-1"));
        assert_eq!(text(mutation, "Calendar", "StartTime").as_deref(), Some("20260824T090000Z"));
        assert_eq!(text(mutation, "Calendar", "EndTime").as_deref(), Some("20260824T100000Z"));
        assert_eq!(text(mutation, "AirSyncBase", "Type").as_deref(), Some("1"));
        assert_eq!(text(mutation, "AirSyncBase", "Data").as_deref(), Some("Agenda"));
        let attendee = mutation
            .descendant("Calendar", "Attendee")
            .ok_or_else(|| anyhow::anyhow!("calendar attendee is missing"))?;
        assert_eq!(text(attendee, "Calendar", "Email").as_deref(), Some("guest@example.invalid"));
        assert_eq!(text(attendee, "Calendar", "Name").as_deref(), Some(""));
        assert_eq!(text(attendee, "Calendar", "AttendeeType").as_deref(), Some("2"));
    }

    let long_id = required_root(&build_meeting_response_long_id(
        "opaque-search-result",
        MeetingResponseChoice::Accept,
    )?)?;
    assert_eq!(text(&long_id, "Search", "LongId").as_deref(), Some("opaque-search-result"));
    assert!(long_id.descendant("MeetingResponse", "CollectionId").is_none());
    assert!(long_id.descendant("MeetingResponse", "RequestId").is_none());
    Ok(())
}

#[test]
fn calendar_delete_and_mutation_response_preserve_sync_state() -> anyhow::Result<()> {
    let request = required_root(&build_calendar_delete("calendar", "41", "server-1")?)?;
    let delete = request
        .descendant("AirSync", "Delete")
        .ok_or_else(|| anyhow::anyhow!("calendar delete is missing"))?;
    assert_eq!(text(delete, "AirSync", "ServerId").as_deref(), Some("server-1"));
    assert!(delete.descendant("AirSync", "ApplicationData").is_none());

    let result = parse_calendar_mutation_sync(&encode(&mutation_response(1, 1, 1))?)?;
    assert_eq!(result.status, 1);
    assert_eq!(result.sync_key.as_deref(), Some("42"));
    assert_eq!(result.server_id.as_deref(), Some("server-2"));
    assert_eq!(parse_calendar_mutation_sync(&encode(&mutation_response(7, 1, 1))?)?.status, 7);
    assert_eq!(parse_calendar_mutation_sync(&encode(&mutation_response(1, 5, 1))?)?.status, 5);
    assert_eq!(parse_calendar_mutation_sync(&encode(&mutation_response(1, 1, 6))?)?.status, 6);
    Ok(())
}

#[test]
fn meeting_response_encodes_choices_and_parses_calendar_id() -> anyhow::Result<()> {
    for (choice, expected) in [
        (MeetingResponseChoice::Accept, "1"),
        (MeetingResponseChoice::Tentative, "2"),
        (MeetingResponseChoice::Decline, "3"),
    ] {
        let request = required_root(&build_meeting_response("calendar", "request-1", choice)?)?;
        assert_eq!(text(&request, "MeetingResponse", "UserResponse").as_deref(), Some(expected));
        assert_eq!(text(&request, "MeetingResponse", "CollectionId").as_deref(), Some("calendar"));
        assert_eq!(text(&request, "MeetingResponse", "RequestId").as_deref(), Some("request-1"));
    }

    let mut root = Element::new("MeetingResponse", "MeetingResponse");
    let mut result = Element::new("MeetingResponse", "Result");
    result.push(Element::text("MeetingResponse", "Status", "1"));
    result.push(Element::text("MeetingResponse", "RequestId", "request-1"));
    result.push(Element::text("MeetingResponse", "CalendarId", "calendar-event-1"));
    root.push(result);
    let parsed = parse_meeting_response(&encode(&root)?)?;
    assert_eq!(parsed.status, 1);
    assert_eq!(parsed.request_id, "request-1");
    assert_eq!(parsed.calendar_id.as_deref(), Some("calendar-event-1"));
    Ok(())
}

#[test]
fn calendar_mutation_builders_reject_incomplete_sources() -> anyhow::Result<()> {
    let item = calendar_item()?;
    assert!(build_calendar_add("", "1", "client", &item).is_err());
    assert!(build_calendar_add("calendar", "", "client", &item).is_err());
    assert!(build_calendar_change("calendar", "1", "", &item).is_err());
    assert!(build_calendar_delete("calendar", "1", "").is_err());
    assert!(build_meeting_response("", "request", MeetingResponseChoice::Accept).is_err());
    assert!(build_meeting_response_long_id("", MeetingResponseChoice::Accept).is_err());
    assert!(parse_calendar_mutation_sync(&[]).is_err());
    assert!(parse_meeting_response(&[]).is_err());
    Ok(())
}

fn calendar_item() -> anyhow::Result<CalendarApplication> {
    let starts_at = Utc
        .with_ymd_and_hms(2026, 8, 24, 9, 0, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("invalid fixture start"))?;
    Ok(CalendarApplication {
        properties: Default::default(),
        time_zone: "AAAA".into(),
        uid: "uid-1".into(),
        dt_stamp: starts_at,
        starts_at,
        ends_at: starts_at + chrono::Duration::hours(1),
        all_day: false,
        subject: "Planning".into(),
        body: "Agenda".into(),
        location: "Room 1".into(),
        reminder_minutes: Some(15),
        busy_status: 2,
        meeting_status: 1,
        response_requested: true,
        attendees: vec![CalendarAttendee {
            email: "guest@example.invalid".into(),
            name: String::new(),
            attendee_type: 2,
            attendee_status: 0,
        }],
    })
}

fn mutation_response(account: u16, collection: u16, item: u16) -> Element {
    let mut root = Element::new("AirSync", "Sync");
    root.push(Element::text("AirSync", "Status", account.to_string()));
    let mut collections = Element::new("AirSync", "Collections");
    let mut collection_element = Element::new("AirSync", "Collection");
    collection_element.push(Element::text("AirSync", "Status", collection.to_string()));
    collection_element.push(Element::text("AirSync", "SyncKey", "42"));
    let mut responses = Element::new("AirSync", "Responses");
    let mut add = Element::new("AirSync", "Add");
    add.push(Element::text("AirSync", "Status", item.to_string()));
    add.push(Element::text("AirSync", "ServerId", "server-2"));
    responses.push(add);
    collection_element.push(responses);
    collections.push(collection_element);
    root.push(collections);
    root
}

fn required_root(data: &[u8]) -> Result<Element, EasError> {
    decode(data)?.ok_or_else(|| EasError::Protocol("expected WBXML document".into()))
}

fn text(root: &Element, namespace: &str, name: &str) -> Option<String> {
    root.descendant(namespace, name).map(Element::text_content)
}
