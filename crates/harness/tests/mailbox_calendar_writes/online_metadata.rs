use super::*;
use anyhow::Context as _;
use eas_mail_protocol::protocol::parse_calendar_item_fetch;
use eas_mail_protocol::wbxml::decode;

#[tokio::test]
async fn adding_online_meeting_attendees_uses_ghosting_and_omits_unchanged_exceptions()
-> anyhow::Result<()> {
    let item = application()?;
    let mut response = decode(&exception_delta::item_response(&item)?)?.context("response")?;
    // Construct a genuine server response, not an illegal outbound Add with read-only fields.
    let mut properties = Element::new("ItemOperations", "Properties");
    let existing = response.descendant("ItemOperations", "Properties").context("properties")?;
    for child in existing.children() {
        properties.push(child.clone());
    }
    for (name, value) in [
        ("OnlineMeetingConfLink", "conf://example.invalid/meeting"),
        ("OnlineMeetingExternalLink", "https://example.invalid/meeting"),
        ("AppointmentReplyTime", "20260824T100000Z"),
    ] {
        properties.push(Element::text("Calendar", name, value));
    }
    let mut recurrence = Element::new("Calendar", "Recurrence");
    for (name, value) in
        [("Type", "1"), ("Interval", "2"), ("DayOfWeek", "2"), ("Occurrences", "13")]
    {
        recurrence.push(Element::text("Calendar", name, value));
    }
    properties.push(recurrence);
    let mut exceptions = Element::new("Calendar", "Exceptions");
    for index in 1..=7 {
        let mut exception = Element::new("Calendar", "Exception");
        exception.push(Element::text(
            "Calendar",
            "ExceptionStartTime",
            (item.starts_at + chrono::Duration::weeks(index * 2))
                .format("%Y%m%dT%H%M%SZ")
                .to_string(),
        ));
        exception.push(Element::new("Calendar", "Reminder"));
        exception.push(Element::new("Calendar", "Categories"));
        exceptions.push(exception);
    }
    properties.push(exceptions);
    let mut fetch = Element::new("ItemOperations", "Fetch");
    fetch.push(Element::text("ItemOperations", "Status", "1"));
    fetch.push(properties);
    response = Element::new("ItemOperations", "ItemOperations");
    response.push(fetch);
    let response = encode(&response)?;
    let parsed = parse_calendar_item_fetch(&response)?.fields.properties.context("parsed")?;
    assert!(parsed.can_write());
    let mut changed = item;
    changed.properties = parsed;
    changed.attendees.push(CalendarAttendee {
        email: "new@example.invalid".into(),
        name: String::new(),
        attendee_type: 1,
        attendee_status: 0,
    });
    let mut expected_delta = changed.clone();
    expected_delta.properties.exceptions.clear();
    let wire = decode(&build_calendar_change("calendar", "key-2", "event", &expected_delta)?)?
        .context("change")?;
    for forbidden in
        ["AppointmentReplyTime", "OnlineMeetingConfLink", "OnlineMeetingExternalLink", "Exception"]
    {
        assert!(wire.descendant("Calendar", forbidden).is_none(), "forbidden {forbidden}");
    }
    // Scripted transport checks initialization, bounded reads and this exact mutation.
    // Its returned local result must retain all three fields and seven unchanged exceptions.
    exception_delta::verify_change(response, changed, expected_delta).await
}
