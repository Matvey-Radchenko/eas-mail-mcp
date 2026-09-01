use std::collections::BTreeMap;

use base64::Engine as _;
use chrono::{NaiveDate, TimeZone as _, Utc};
use eas_mail_protocol::{CalendarAttendee, CalendarFields, Patch};

use super::timezone::EventTimeZone;
use super::{AgendaRange, local_to_utc, plan, recurrence};
use crate::ErrorCode;
use crate::backend::BackendEvent;
use crate::model::CalendarSearchInput;

#[test]
fn agenda_filters_compact_fields_and_sorts_occurrences() -> anyhow::Result<()> {
    let mut matching = event("2026-08-04T09:00:00Z", "2026-08-04T10:00:00Z")?;
    matching.fields.location = Patch::Value("Room A".into());
    matching.fields.organizer = Patch::Value("Organizer".into());
    matching.fields.organizer_email = Patch::Value("organizer@example.invalid".into());
    matching.fields.attendees = Patch::Value(vec![CalendarAttendee {
        email: "alice@example.invalid".into(),
        name: "Alice".into(),
        attendee_type: 1,
        attendee_status: 3,
    }]);
    let other = event("2026-08-03T09:00:00Z", "2026-08-03T10:00:00Z")?;
    let input = CalendarSearchInput {
        query: Some("ALICE".into()),
        date_from: Some("2026-08-03".into()),
        date_to: Some("2026-08-05".into()),
        time_zone: Some("UTC".into()),
        account_ids: None,
        limit: None,
    };

    let events = plan(&input)?.apply(vec![matching, other])?;
    assert_eq!(events.len(), 1);
    let event = events.first().ok_or_else(|| anyhow::anyhow!("matching event is missing"))?;
    assert_eq!(event.fields.location, Patch::Value("Room A".into()));
    Ok(())
}

#[test]
fn agenda_rejects_invalid_dates_timezones_and_local_times() -> anyhow::Result<()> {
    for input in [
        agenda_input("bad-date", "2026-08-05", "UTC"),
        agenda_input("2026-08-05", "2026-08-03", "UTC"),
        agenda_input("+262142-12-31", "+262142-12-31", "UTC"),
        agenda_input("2026-08-03", "2026-08-05", "Not/AZone"),
    ] {
        assert_eq!(
            plan(&input).err().map(|error| error.envelope.code),
            Some(ErrorCode::ValidationFailed)
        );
    }

    let nonexistent = NaiveDate::from_ymd_opt(2026, 3, 29)
        .and_then(|date| date.and_hms_opt(2, 30, 0))
        .ok_or_else(|| anyhow::anyhow!("nonexistent-time fixture is invalid"))?;
    let ambiguous = NaiveDate::from_ymd_opt(2026, 10, 25)
        .and_then(|date| date.and_hms_opt(2, 30, 0))
        .ok_or_else(|| anyhow::anyhow!("ambiguous-time fixture is invalid"))?;
    assert!(local_to_utc(chrono_tz::Europe::Belgrade, nonexistent).is_err());
    assert!(local_to_utc(chrono_tz::Europe::Belgrade, ambiguous).is_err());
    Ok(())
}

#[test]
fn malformed_recurrence_is_rejected_before_output() -> anyhow::Result<()> {
    let invalid_patterns = [
        fields([("calendartype", "2"), ("type", "0")]),
        fields([("type", "bad"), ("interval", "1")]),
        fields([("type", "4"), ("interval", "1")]),
        fields([("type", "0"), ("interval", "1000")]),
        fields([("type", "1"), ("firstdayofweek", "7")]),
        fields([("type", "1"), ("dayofweek", "0")]),
        fields([("type", "0"), ("until", "not-a-time")]),
    ];
    for pattern in invalid_patterns {
        assert!(expand_with(pattern)?.is_err());
    }
    for pattern in [
        fields([("type", "1")]),
        fields([("type", "2")]),
        fields([("type", "3"), ("dayofweek", "2")]),
        fields([("type", "5"), ("dayofmonth", "3")]),
        fields([("type", "6"), ("monthofyear", "8")]),
    ] {
        assert!(expand_with(pattern)?.is_err());
    }
    let mut missing_type = event("2026-08-03T09:00:00Z", "2026-08-03T10:00:00Z")?;
    missing_type.fields.recurrence =
        Patch::Value(BTreeMap::from([("interval".into(), "1".into())]));
    assert!(recurrence::expand(missing_type, range()?, chrono_tz::UTC).is_err());
    Ok(())
}

#[test]
fn malformed_event_exceptions_and_eas_timezones_are_rejected() -> anyhow::Result<()> {
    let mut missing_start = recurring_event()?;
    missing_start.fields.starts_at = Patch::Missing;
    assert!(recurrence::expand(missing_start, range()?, chrono_tz::UTC).is_err());

    let mut missing_end = recurring_event()?;
    missing_end.fields.ends_at = Patch::Missing;
    assert!(recurrence::expand(missing_end, range()?, chrono_tz::UTC).is_err());

    let mut bad_duration = recurring_event()?;
    bad_duration.fields.ends_at = bad_duration.fields.starts_at.clone();
    assert!(recurrence::expand(bad_duration, range()?, chrono_tz::UTC).is_err());

    for exception in [
        BTreeMap::from([("deleted".into(), "1".into())]),
        BTreeMap::from([("exceptionstarttime".into(), "bad".into())]),
        BTreeMap::from([
            ("exceptionstarttime".into(), "20260804T090000Z".into()),
            ("starttime".into(), "20260804T120000Z".into()),
            ("endtime".into(), "20260804T110000Z".into()),
        ]),
    ] {
        let mut event = recurring_event()?;
        event.fields.exceptions = Patch::Value(vec![exception]);
        assert!(recurrence::expand(event, range()?, chrono_tz::UTC).is_err());
    }

    assert!(EventTimeZone::parse(Some("%%%"), chrono_tz::UTC).is_err());
    assert!(EventTimeZone::parse(Some(&encoded(&[0; 171])), chrono_tz::UTC).is_err());
    let mut excessive_offset = [0_u8; 172];
    excessive_offset[0..4].copy_from_slice(&2_000_i32.to_le_bytes());
    assert!(EventTimeZone::parse(Some(&encoded(&excessive_offset)), chrono_tz::UTC).is_err());
    let mut excessive_daylight_offset = [0_u8; 172];
    excessive_daylight_offset[168..172].copy_from_slice(&2_000_i32.to_le_bytes());
    assert!(
        EventTimeZone::parse(Some(&encoded(&excessive_daylight_offset)), chrono_tz::UTC).is_err()
    );
    let mut invalid_dst = [0_u8; 172];
    set_u16(&mut invalid_dst, 70, 13);
    set_u16(&mut invalid_dst, 74, 1);
    assert!(EventTimeZone::parse(Some(&encoded(&invalid_dst)), chrono_tz::UTC).is_err());
    let mut incomplete_dst = [0_u8; 172];
    set_u16(&mut incomplete_dst, 70, 10);
    set_u16(&mut incomplete_dst, 72, 0);
    set_u16(&mut incomplete_dst, 74, 5);
    assert!(EventTimeZone::parse(Some(&encoded(&incomplete_dst)), chrono_tz::UTC).is_err());

    let no_dst = EventTimeZone::parse(Some(&encoded(&[0; 172])), chrono_tz::UTC)?;
    let instant = Utc
        .with_ymd_and_hms(2026, 8, 3, 9, 0, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("timezone fixture is invalid"))?;
    let local = no_dst.to_local(instant)?;
    assert_eq!(no_dst.to_utc(local)?, instant);
    Ok(())
}

fn agenda_input(from: &str, to: &str, zone: &str) -> CalendarSearchInput {
    CalendarSearchInput {
        query: None,
        date_from: Some(from.into()),
        date_to: Some(to.into()),
        time_zone: Some(zone.into()),
        account_ids: None,
        limit: None,
    }
}

fn expand_with(
    pattern: BTreeMap<String, String>,
) -> anyhow::Result<crate::Result<Vec<BackendEvent>>> {
    let mut event = event("2026-08-03T09:00:00Z", "2026-08-03T10:00:00Z")?;
    event.fields.recurrence = Patch::Value(pattern);
    Ok(recurrence::expand(event, range()?, chrono_tz::UTC))
}

fn recurring_event() -> anyhow::Result<BackendEvent> {
    let mut event = event("2026-08-03T09:00:00Z", "2026-08-03T10:00:00Z")?;
    event.fields.recurrence = Patch::Value(fields([("type", "0"), ("interval", "1")]));
    Ok(event)
}

fn event(start: &str, end: &str) -> anyhow::Result<BackendEvent> {
    Ok(BackendEvent {
        occurrence_start: None,
        account_id: "work".into(),
        long_id: String::new(),
        collection_id: Some("calendar".into()),
        server_id: Some("event".into()),
        fields: CalendarFields {
            subject: Patch::Value("Planning".into()),
            starts_at: Patch::Value(Some(start.parse()?)),
            ends_at: Patch::Value(Some(end.parse()?)),
            all_day: Patch::Value(false),
            recurrence: Patch::Value(BTreeMap::new()),
            exceptions: Patch::Value(Vec::new()),
            ..CalendarFields::default()
        },
    })
}

fn fields<const N: usize>(values: [(&str, &str); N]) -> BTreeMap<String, String> {
    values.into_iter().map(|(key, value)| (key.into(), value.into())).collect()
}

fn range() -> anyhow::Result<AgendaRange> {
    Ok(AgendaRange {
        start: Utc
            .with_ymd_and_hms(2026, 8, 3, 0, 0, 0)
            .single()
            .ok_or_else(|| anyhow::anyhow!("range start is invalid"))?,
        end: Utc
            .with_ymd_and_hms(2026, 8, 6, 0, 0, 0)
            .single()
            .ok_or_else(|| anyhow::anyhow!("range end is invalid"))?,
    })
}

fn encoded(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn set_u16(bytes: &mut [u8], offset: usize, value: u16) {
    if let Some(target) = bytes.get_mut(offset..offset + 2) {
        target.copy_from_slice(&value.to_le_bytes());
    }
}
