use anyhow::Context as _;
use eas_mail_protocol::CollectionKind;
use eas_mail_protocol::protocol::build_sync;
use eas_mail_protocol::wbxml::decode;
use std::collections::BTreeSet;

#[test]
fn calendar_initialization_ghosts_server_properties_and_advertises_required_fields()
-> anyhow::Result<()> {
    let tree = decode(&build_sync("calendar", "0", CollectionKind::Calendar, 6, 0)?)?
        .context("request")?;
    let collection = tree.descendant("AirSync", "Collection").context("collection")?;
    assert_eq!(
        collection.children().map(|node| node.name.as_str()).collect::<Vec<_>>(),
        ["SyncKey", "CollectionId", "Supported"]
    );
    let supported = collection.child("AirSync", "Supported").context("supported")?;
    let fields = supported
        .children()
        .map(|node| (node.namespace.as_str(), node.name.as_str()))
        .collect::<BTreeSet<_>>();
    for name in [
        "DtStamp",
        "Categories",
        "Sensitivity",
        "BusyStatus",
        "UID",
        "TimeZone",
        "StartTime",
        "Subject",
        "Location",
        "EndTime",
        "Recurrence",
        "AllDayEvent",
        "Reminder",
        "Exceptions",
        "Attendees",
        "MeetingStatus",
        "ResponseRequested",
    ] {
        assert!(fields.contains(&("Calendar", name)), "required/writable field {name}");
    }
    assert_eq!(fields.len(), 17);
    for name in [
        "AppointmentReplyTime",
        "OnlineMeetingConfLink",
        "OnlineMeetingExternalLink",
        "OrganizerEmail",
        "OrganizerName",
    ] {
        assert!(!fields.contains(&("Calendar", name)), "server-managed field {name}");
    }
    for (kind, key) in [(CollectionKind::Calendar, "next"), (CollectionKind::Mail, "0")] {
        let tree = decode(&build_sync("folder", key, kind, 6, 0)?)?.context("request")?;
        assert!(tree.descendant("AirSync", "Supported").is_none());
    }
    Ok(())
}
