use anyhow::Context as _;
use chrono::{DateTime, Utc};
use eas_mail_protocol::protocol::{
    build_calendar_add, build_calendar_change, build_meeting_response_instance,
    parse_calendar_item_fetch,
};
use eas_mail_protocol::wbxml::{Element, decode, encode};
use eas_mail_protocol::{
    CalendarApplication, CalendarException, CalendarFields, CalendarProperties, CalendarRecurrence,
    Patch, RecurrenceEnd, RecurrencePattern,
};
use std::collections::BTreeMap;

#[test]
fn every_gregorian_rule_round_trips_without_unknown_field_loss() -> anyhow::Result<()> {
    for pattern in [
        RecurrencePattern::Daily,
        RecurrencePattern::Weekly { days: 42 },
        RecurrencePattern::Monthly { day: 31 },
        RecurrencePattern::MonthlyRelative { days: 2, week: 5 },
        RecurrencePattern::Yearly { month: 2, day: 29 },
        RecurrencePattern::YearlyRelative { month: 11, days: 62, week: 3 },
    ] {
        for end in [
            RecurrenceEnd::Never,
            RecurrenceEnd::Count(5),
            RecurrenceEnd::Until(time("2026-08-31T10:00:00Z")?),
        ] {
            let value = CalendarRecurrence {
                pattern: pattern.clone(),
                interval: 2,
                first_day_of_week: 1,
                end,
            };
            value.validate()?;
            assert_eq!(CalendarRecurrence::from_fields(&value.to_fields())?, value);
        }
    }
    for fields in [
        vec![("type", "1")],
        vec![("type", "0"), ("interval", "0")],
        vec![("type", "0"), ("until", "bad")],
        vec![("type", "0"), ("occurrences", "1"), ("until", "20260831T100000Z")],
        vec![("type", "0"), ("calendartype", "6")],
        vec![("type", "0"), ("unknown", "1")],
    ] {
        let fields: BTreeMap<String, String> =
            fields.into_iter().map(|(key, value)| (key.into(), value.into())).collect();
        assert!(CalendarRecurrence::from_fields(&fields).is_err());
    }
    Ok(())
}

#[test]
fn recurrence_exceptions_survive_wbxml_fetch_and_change() -> anyhow::Result<()> {
    let mut item = application()?;
    item.properties.exceptions.push(CalendarException {
        original_start: time("2026-08-25T10:00:00Z")?,
        deleted: false,
        fields: CalendarFields {
            subject: Patch::Value(String::new()),
            body: Patch::Value("Override body".into()),
            starts_at: Patch::Value(Some(time("2026-08-25T11:00:00Z")?)),
            ends_at: Patch::Value(Some(time("2026-08-25T12:00:00Z")?)),
            ..Default::default()
        },
    });
    item.properties.exceptions.push(CalendarException {
        original_start: time("2026-08-26T10:00:00Z")?,
        deleted: true,
        fields: CalendarFields::default(),
    });
    let encoded = build_calendar_add("calendar", "key", "client", &item)?;
    let tree = decode(&encoded)?.context("request")?;
    let properties = tree.descendant("AirSync", "ApplicationData").context("application")?.clone();
    let parsed = fetch(properties)?;
    let properties = parsed.properties.context("write metadata")?;
    assert!(properties.can_write());
    assert_eq!(properties.recurrence, item.properties.recurrence);
    assert_eq!(properties.exceptions.len(), 2);
    assert_eq!(
        properties.exceptions.first().context("override")?.fields.subject,
        Patch::Value(String::new())
    );
    assert_eq!(
        properties.exceptions.first().context("override")?.fields.body,
        Patch::Value("Override body".into())
    );
    item.properties = properties;
    assert!(build_calendar_change("calendar", "key", "server", &item).is_ok());
    item.properties.unsupported = true;
    assert!(build_calendar_change("calendar", "key", "server", &item).is_err());
    Ok(())
}

#[test]
fn original_instance_id_is_used_only_for_occurrence_response() -> anyhow::Result<()> {
    let original = time("2026-08-25T10:00:00Z")?;
    for selected in [None, Some(original)] {
        let body = build_meeting_response_instance(
            "calendar",
            "item",
            eas_mail_protocol::MeetingResponseChoice::Accept,
            selected,
        )?;
        let tree = decode(&body)?.context("request")?;
        assert_eq!(
            tree.descendant("MeetingResponse", "InstanceId").map(Element::text_content),
            selected.map(|_| "2026-08-25T10:00:00.000Z".to_owned())
        );
    }
    Ok(())
}

#[test]
fn recurring_add_matches_the_reviewable_wire_golden() -> anyhow::Result<()> {
    let mut item = application()?;
    for (start, deleted) in [("2026-08-25T10:00:00Z", false), ("2026-08-26T10:00:00Z", true)] {
        item.properties.exceptions.push(CalendarException {
            original_start: time(start)?,
            deleted,
            fields: if deleted {
                CalendarFields::default()
            } else {
                CalendarFields { subject: Patch::Value("Changed".into()), ..Default::default() }
            },
        });
    }
    assert_eq!(
        build_calendar_add("calendar", "key", "client", &item)?,
        include_bytes!("../../../fixtures/eas/calendar-series/request.wbxml")
    );
    let result = eas_mail_protocol::protocol::parse_calendar_mutation_sync(include_bytes!(
        "../../../fixtures/eas/calendar-series/response.wbxml"
    ))?;
    assert_eq!(result.server_id.as_deref(), Some("series-item"));
    Ok(())
}

#[test]
fn unsupported_fields_do_not_break_reads_but_block_full_writes() -> anyhow::Result<()> {
    let mut properties = Element::new("ItemOperations", "Properties");
    properties.push(Element::text(
        "Calendar",
        "OnlineMeetingExternalLink",
        "https://example.invalid/meeting",
    ));
    let fields = fetch(properties)?;
    assert!(!fields.properties.context("properties")?.can_write());
    Ok(())
}

#[test]
fn absent_and_cleared_exception_categories_and_sensitivity_are_distinct() -> anyhow::Result<()> {
    let mut item = application()?;
    item.properties.sensitivity = Some(2);
    item.properties.categories = Some(vec!["Private category".into()]);
    for (index, properties) in [
        None,
        Some(CalendarProperties {
            sensitivity: Some(0),
            categories: Some(Vec::new()),
            ..Default::default()
        }),
    ]
    .into_iter()
    .enumerate()
    {
        item.properties.exceptions.push(CalendarException {
            original_start: item.starts_at + chrono::Duration::days(i64::try_from(index)?),
            deleted: false,
            fields: CalendarFields {
                properties,
                subject: Patch::Value("Override".into()),
                ..Default::default()
            },
        });
    }
    let tree =
        decode(&build_calendar_change("calendar", "key", "server", &item)?)?.context("request")?;
    let fetched =
        fetch(tree.descendant("AirSync", "ApplicationData").context("properties")?.clone())?;
    let properties = fetched.properties.context("typed")?;
    assert_eq!(properties.sensitivity, Some(2));
    let inherited = properties
        .exceptions
        .first()
        .context("inherited")?
        .fields
        .properties
        .as_ref()
        .context("properties")?;
    assert_eq!(inherited.sensitivity, None);
    assert_eq!(inherited.categories, None);
    let cleared = properties
        .exceptions
        .get(1)
        .context("cleared")?
        .fields
        .properties
        .as_ref()
        .context("properties")?;
    assert_eq!(cleared.sensitivity, Some(0));
    assert_eq!(cleared.categories, Some(Vec::new()));
    Ok(())
}

#[test]
fn malformed_calendar_scalars_attendees_and_nested_rules_are_readable_but_not_writable()
-> anyhow::Result<()> {
    for (name, value) in [
        ("AllDayEvent", "2"),
        ("BusyStatus", "bad"),
        ("StartTime", "bad"),
        ("Sensitivity", "4"),
        ("ResponseType", "9"),
    ] {
        let mut fields = Element::new("ItemOperations", "Properties");
        fields.push(Element::text("Calendar", name, value));
        assert!(!fetch(fields)?.properties.context("properties")?.can_write());
    }
    for field in [
        Element::text("Calendar", "Name", "missing email"),
        Element::text("Calendar", "AttendeeType", "4"),
        Element::text("Calendar", "AttendeeStatus", "9"),
        Element::text("Calendar", "Subject", "unknown"),
    ] {
        let mut attendee = Element::new("Calendar", "Attendee");
        if field.name != "Name" {
            attendee.push(Element::text("Calendar", "Email", "guest@example.invalid"));
        }
        attendee.push(field);
        let mut attendees = Element::new("Calendar", "Attendees");
        attendees.push(attendee);
        let mut fields = Element::new("ItemOperations", "Properties");
        fields.push(attendees);
        assert!(!fetch(fields)?.properties.context("properties")?.can_write());
    }
    let mut nested = Element::new("Calendar", "Exception");
    nested.push(Element::text("Calendar", "ExceptionStartTime", "20260824T100000Z"));
    nested.push(Element::text("Calendar", "UID", "not allowed in EAS 14.1 exceptions"));
    let mut exceptions = Element::new("Calendar", "Exceptions");
    exceptions.push(nested.clone());
    exceptions.push(nested);
    let mut fields = Element::new("ItemOperations", "Properties");
    fields.push(exceptions);
    assert!(!fetch(fields)?.properties.context("properties")?.can_write());
    Ok(())
}

#[test]
fn recurrence_and_exception_bounds_fail_closed() -> anyhow::Result<()> {
    for fields in [
        vec![("type", "7")],
        vec![("type", "0"), ("interval", "1000")],
        vec![("type", "0"), ("firstdayofweek", "7")],
        vec![("type", "0"), ("occurrences", "0")],
        vec![("type", "1"), ("dayofweek", "0")],
        vec![("type", "2"), ("dayofmonth", "32")],
        vec![("type", "3"), ("dayofweek", "2"), ("weekofmonth", "6")],
        vec![("type", "5"), ("monthofyear", "13"), ("dayofmonth", "1")],
        vec![("type", "0"), ("dayofmonth", "1")],
        vec![("type", "0"), ("isleapmonth", "1")],
    ] {
        let fields = fields.into_iter().map(|(key, value)| (key.into(), value.into())).collect();
        assert!(CalendarRecurrence::from_fields(&fields).is_err());
    }
    let mut item = application()?;
    let exception = CalendarException {
        original_start: item.starts_at,
        deleted: true,
        fields: CalendarFields::default(),
    };
    item.properties.exceptions = vec![exception; 257];
    assert!(build_calendar_add("calendar", "key", "client", &item).is_err());
    Ok(())
}

#[test]
fn only_retained_nonempty_attendee_overrides_make_a_series_a_meeting() -> anyhow::Result<()> {
    let mut item = application()?;
    assert!(!item.properties.has_attendee_overrides());
    item.properties.exceptions.push(CalendarException {
        original_start: item.starts_at,
        deleted: false,
        fields: CalendarFields {
            attendees: Patch::Value(vec![eas_mail_protocol::CalendarAttendee {
                email: "guest@example.invalid".into(),
                name: String::new(),
                attendee_type: 1,
                attendee_status: 0,
            }]),
            ..Default::default()
        },
    });
    assert!(item.properties.has_attendee_overrides());
    item.properties.exceptions.first_mut().context("exception")?.deleted = true;
    assert!(!item.properties.has_attendee_overrides());
    let exception = item.properties.exceptions.first_mut().context("exception")?;
    exception.deleted = false;
    exception.fields.attendees = Patch::Value(vec![]);
    assert!(!item.properties.has_attendee_overrides());
    Ok(())
}

fn fetch(mut properties: Element) -> anyhow::Result<CalendarFields> {
    properties.namespace = "ItemOperations".into();
    properties.name = "Properties".into();
    let mut fetch = Element::new("ItemOperations", "Fetch");
    fetch.push(Element::text("ItemOperations", "Status", "1"));
    fetch.push(properties);
    let mut response = Element::new("ItemOperations", "Response");
    response.push(fetch);
    let mut root = Element::new("ItemOperations", "ItemOperations");
    root.push(response);
    Ok(parse_calendar_item_fetch(&encode(&root)?)?.fields)
}

fn application() -> anyhow::Result<CalendarApplication> {
    Ok(CalendarApplication {
        properties: CalendarProperties {
            recurrence: Some(CalendarRecurrence {
                pattern: RecurrencePattern::Daily,
                interval: 1,
                first_day_of_week: 1,
                end: RecurrenceEnd::Count(5),
            }),
            ..Default::default()
        },
        time_zone: format!("{}==", "A".repeat(230)),
        uid: "series@example.invalid".into(),
        dt_stamp: time("2026-08-01T00:00:00Z")?,
        starts_at: time("2026-08-24T10:00:00Z")?,
        ends_at: time("2026-08-24T11:00:00Z")?,
        all_day: false,
        subject: "Series".into(),
        body: "Body".into(),
        location: "Room".into(),
        reminder_minutes: Some(15),
        busy_status: 2,
        meeting_status: 0,
        response_requested: false,
        attendees: Vec::new(),
    })
}

fn time(value: &str) -> anyhow::Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc))
}
