#![expect(
    clippy::indexing_slicing,
    reason = "fixed test fixtures use direct indexing for readable assertions"
)]

use eas_mail_protocol::protocol::{
    build_folder_sync, build_mark_read, build_policy_ack, build_wipe_ack, parse_folder_sync,
    parse_mutation_sync, parse_provision, parse_sync,
};
use eas_mail_protocol::wbxml::{Element, decode, encode};
use eas_mail_protocol::{
    CalendarAttendee, ChangeData, ChangeKind, CollectionKind, EasError, Patch,
};

#[test]
fn calendar_sync_preserves_recurrence_exceptions_and_order() -> eas_mail_protocol::Result<()> {
    let response = sync_response()?;
    let page = parse_sync(&response, CollectionKind::Calendar)?;
    assert_eq!(page.account_status, 5);
    assert_eq!(page.collection_status, 1);
    assert_eq!(page.sync_key, "next-key");
    assert!(page.more_available);
    assert_eq!(
        page.changes.iter().map(|change| change.kind).collect::<Vec<_>>(),
        [ChangeKind::Add, ChangeKind::Change, ChangeKind::Delete, ChangeKind::SoftDelete,]
    );
    let ChangeData::Calendar(fields) = &page.changes[0].data else {
        return Err(EasError::Protocol("calendar data is missing".into()));
    };
    assert_eq!(fields.subject, Patch::Value("Planning".into()));
    assert_eq!(fields.organizer, Patch::Value("organizer@example.com".into()));
    assert_eq!(
        fields.attendees,
        Patch::Value(vec![CalendarAttendee {
            email: "guest@example.com".into(),
            name: String::new(),
            attendee_type: 1,
            attendee_status: 0,
        }])
    );
    assert_eq!(fields.reminder_minutes, Patch::Value(Some(15)));
    assert_eq!(fields.meeting_status, Patch::Value(3));
    let ChangeData::Calendar(change) = &page.changes[1].data else {
        return Err(EasError::Protocol("calendar change data is missing".into()));
    };
    assert_eq!(change.reminder_minutes, Patch::Value(None));
    assert!(change.properties.as_ref().is_some_and(|value| value.can_write()));
    let Patch::Value(recurrence) = &fields.recurrence else {
        return Err(EasError::Protocol("recurrence is missing".into()));
    };
    assert_eq!(recurrence.get("type").map(String::as_str), Some("1"));
    let Patch::Value(exceptions) = &fields.exceptions else {
        return Err(EasError::Protocol("exceptions are missing".into()));
    };
    assert_eq!(exceptions[0].get("deleted").map(String::as_str), Some("1"));
    Ok(())
}

#[test]
fn sync_and_mark_read_handle_initial_and_mutation_shapes() -> eas_mail_protocol::Result<()> {
    let initial =
        eas_mail_protocol::protocol::build_sync("calendar", "0", CollectionKind::Calendar, 6, 500)?;
    let root = required_root(&initial)?;
    assert!(root.descendant("AirSync", "Options").is_none());

    for (is_read, expected) in [(true, "1"), (false, "0")] {
        let request = required_root(&build_mark_read("inbox", "mail-1", "42", is_read)?)?;
        assert_eq!(text(&request, "Email", "Read"), Some(expected.into()));
        assert_eq!(text(&request, "AirSync", "GetChanges"), Some("0".into()));
    }
    let mutation = mutation_response()?;
    let result = parse_mutation_sync(&mutation)?;
    assert_eq!(result.status, 6);
    assert_eq!(result.sync_key.as_deref(), Some("43"));
    Ok(())
}

#[test]
fn sync_parser_rejects_empty_and_missing_collection() -> eas_mail_protocol::Result<()> {
    assert!(parse_sync(&[], CollectionKind::Mail).is_err());
    let root = Element::new("AirSync", "Sync");
    assert!(parse_sync(&encode(&root)?, CollectionKind::Mail).is_err());
    Ok(())
}

#[test]
fn folder_sync_parses_all_supported_types_and_deletions() -> eas_mail_protocol::Result<()> {
    let request = required_root(&build_folder_sync("12")?)?;
    assert_eq!(text(&request, "FolderHierarchy", "SyncKey"), Some("12".into()));
    assert!(parse_folder_sync(&[]).is_err());

    let mut root = Element::new("FolderHierarchy", "FolderSync");
    root.push(Element::text("FolderHierarchy", "Status", "1"));
    root.push(Element::text("FolderHierarchy", "SyncKey", "13"));
    let mut changes = Element::new("FolderHierarchy", "Changes");
    for (name, id, folder_type) in
        [("Add", "inbox", 2), ("Update", "calendar", 8), ("Add", "contacts", 9)]
    {
        let mut command = Element::new("FolderHierarchy", name);
        command.push(Element::text("FolderHierarchy", "ServerId", id));
        command.push(Element::text("FolderHierarchy", "DisplayName", id));
        command.push(Element::text("FolderHierarchy", "Type", folder_type.to_string()));
        changes.push(command);
    }
    let mut ignored = Element::new("FolderHierarchy", "Add");
    ignored.push(Element::text("FolderHierarchy", "Type", "2"));
    changes.push(ignored);
    let mut delete = Element::new("FolderHierarchy", "Delete");
    delete.push(Element::text("FolderHierarchy", "ServerId", "old"));
    changes.push(delete);
    changes.push(Element::new("FolderHierarchy", "Delete"));
    root.push(changes);
    let page = parse_folder_sync(&encode(&root)?)?;
    assert_eq!(page.folders.len(), 3);
    assert_eq!(
        page.folders
            .iter()
            .find(|folder| folder.server_id == "inbox")
            .and_then(|folder| folder.kind),
        Some(CollectionKind::Mail)
    );
    assert_eq!(
        page.folders
            .iter()
            .find(|folder| folder.server_id == "calendar")
            .and_then(|folder| folder.kind),
        Some(CollectionKind::Calendar)
    );
    assert_eq!(
        page.folders
            .iter()
            .find(|folder| folder.server_id == "contacts")
            .and_then(|folder| folder.kind),
        None
    );
    assert_eq!(page.deleted_ids, ["old"]);
    Ok(())
}

#[test]
fn provision_builders_and_parser_cover_policy_and_wipe_forms() -> eas_mail_protocol::Result<()> {
    for (supported, status) in [(true, "1"), (false, "2")] {
        let root = required_root(&build_policy_ack(77, supported)?)?;
        assert_eq!(text(&root, "Provision", "PolicyKey"), Some("77".into()));
        assert_eq!(text(&root, "Provision", "Status"), Some(status.into()));
    }
    for account_only in [false, true] {
        let root = required_root(&build_wipe_ack(account_only)?)?;
        let name = if account_only { "AccountOnlyRemoteWipe" } else { "RemoteWipe" };
        let wipe = root
            .descendant("Provision", name)
            .ok_or_else(|| EasError::Protocol("wipe acknowledgement is missing".into()))?;
        assert_eq!(wipe.child("Provision", "Status").map(Element::text_content), Some("1".into()));
    }
    assert!(parse_provision(&[]).is_err());

    let mut root = Element::new("Provision", "Provision");
    root.push(Element::text("Provision", "Status", "1"));
    root.push(Element::new("Provision", "RemoteWipe"));
    root.push(Element::new("Provision", "AccountOnlyRemoteWipe"));
    let parsed = parse_provision(&encode(&root)?)?;
    assert!(parsed.remote_wipe && parsed.account_only_remote_wipe);
    assert_eq!(parsed.policy_key, None);
    Ok(())
}

fn sync_response() -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("AirSync", "Sync");
    root.push(Element::text("AirSync", "Status", "5"));
    let mut collections = Element::new("AirSync", "Collections");
    let mut collection = Element::new("AirSync", "Collection");
    collection.push(Element::text("AirSync", "SyncKey", "next-key"));
    collection.push(Element::text("AirSync", "Status", "1"));
    collection.push(Element::new("AirSync", "MoreAvailable"));
    let mut commands = Element::new("AirSync", "Commands");
    commands.push(calendar_command("Add", "event-1", true));
    commands.push(calendar_command("Change", "event-2", false));
    commands.push(id_command("Delete", "event-3"));
    commands.push(id_command("SoftDelete", "event-4"));
    commands.push(Element::new("AirSync", "Fetch"));
    commands.push(Element::new("AirSync", "Add"));
    collection.push(commands);
    collections.push(collection);
    root.push(collections);
    encode(&root)
}

fn calendar_command(name: &str, id: &str, complete: bool) -> Element {
    let mut command = id_command(name, id);
    let mut data = Element::new("AirSync", "ApplicationData");
    data.push(Element::text("Calendar", "Subject", if complete { "Planning" } else { "" }));
    if !complete {
        data.push(Element::new("Calendar", "Reminder"));
    }
    if complete {
        data.push(Element::text("Calendar", "StartTime", "20260102T030405Z"));
        data.push(Element::text("Calendar", "EndTime", "2026-01-02T04:04:05Z"));
        data.push(Element::text("Calendar", "AllDayEvent", "0"));
        data.push(Element::text("Calendar", "Location", "Room 1"));
        data.push(Element::text("Calendar", "OrganizerEmail", "organizer@example.com"));
        data.push(Element::text("Calendar", "Reminder", "15"));
        data.push(Element::text("Calendar", "MeetingStatus", "3"));
        let mut attendees = Element::new("Calendar", "Attendees");
        let mut attendee = Element::new("Calendar", "Attendee");
        attendee.push(Element::text("Calendar", "Email", "guest@example.com"));
        attendees.push(attendee);
        attendees.push(Element::new("Calendar", "BusyStatus"));
        data.push(attendees);
        let mut recurrence = Element::new("Calendar", "Recurrence");
        recurrence.push(Element::text("Calendar", "Type", "1"));
        data.push(recurrence);
        let mut exceptions = Element::new("Calendar", "Exceptions");
        let mut exception = Element::new("Calendar", "Exception");
        exception.push(Element::text("Calendar", "Deleted", "1"));
        exceptions.push(exception);
        exceptions.push(Element::new("Calendar", "TimeZone"));
        data.push(exceptions);
    }
    command.push(data);
    command
}

fn id_command(name: &str, id: &str) -> Element {
    let mut command = Element::new("AirSync", name);
    command.push(Element::text("AirSync", "ServerId", id));
    command
}

fn mutation_response() -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("AirSync", "Sync");
    let mut collection = Element::new("AirSync", "Collection");
    collection.push(Element::text("AirSync", "SyncKey", "43"));
    let mut responses = Element::new("AirSync", "Responses");
    let mut change = Element::new("AirSync", "Change");
    change.push(Element::text("AirSync", "Status", "6"));
    responses.push(change);
    collection.push(responses);
    let mut collections = Element::new("AirSync", "Collections");
    collections.push(collection);
    root.push(collections);
    encode(&root)
}

fn required_root(data: &[u8]) -> eas_mail_protocol::Result<Element> {
    decode(data)?.ok_or_else(|| EasError::Protocol("expected WBXML document".into()))
}

fn text(root: &Element, namespace: &str, name: &str) -> Option<String> {
    root.descendant(namespace, name).map(Element::text_content)
}
