use std::collections::BTreeMap;

use eas_mail_protocol::{CalendarFields, Patch};

use super::{AgendaRange, plan, recurrence};
use crate::ErrorCode;
use crate::backend::BackendEvent;
use crate::model::{CalendarScheduleInput, CalendarSearchInput};

#[test]
fn search_plan_requires_text_or_complete_bounded_range() -> anyhow::Result<()> {
    let text = plan(&input(Some("planning"), None, None, None))?;
    assert!(!text.uses_agenda_scan());
    let agenda = plan(&input(None, Some("2026-08-03"), Some("2026-08-09"), Some("UTC")))?;
    assert!(agenda.uses_agenda_scan());

    for invalid in [
        input(None, None, None, None),
        input(Some(" "), None, None, None),
        input(None, Some("2026-08-03"), None, Some("UTC")),
        input(None, Some("2026-08-03"), Some("2026-09-03"), Some("UTC")),
    ] {
        let error = plan(&invalid).err().ok_or_else(|| anyhow::anyhow!("input should fail"))?;
        assert_eq!(error.envelope.code, ErrorCode::ValidationFailed);
    }
    Ok(())
}

#[test]
fn weekly_recurrence_expands_only_inside_the_requested_range() -> anyhow::Result<()> {
    let mut event = event_at("2026-08-03T09:00:00Z", "2026-08-03T10:00:00Z")?;
    event.fields.recurrence = recurrence([("type", "1"), ("dayofweek", "2")]);
    let range = range("2026-08-10T00:00:00Z", "2026-08-17T00:00:00Z")?;
    let events = recurrence::expand(event, range, chrono_tz::UTC)?;
    assert_eq!(
        event_times(&events),
        vec![("2026-08-10T09:00:00+00:00".into(), "2026-08-10T10:00:00+00:00".into())]
    );
    Ok(())
}

#[test]
fn recurring_wall_time_survives_dst_transition() -> anyhow::Result<()> {
    let mut event = event_at("2026-03-23T08:00:00Z", "2026-03-23T09:00:00Z")?;
    event.fields.recurrence = recurrence([("type", "0"), ("interval", "1")]);
    let range = range("2026-03-29T00:00:00Z", "2026-03-30T00:00:00Z")?;
    let events = recurrence::expand(event, range, chrono_tz::Europe::Belgrade)?;
    assert_eq!(
        event_times(&events),
        vec![("2026-03-29T07:00:00+00:00".into(), "2026-03-29T08:00:00+00:00".into())]
    );
    Ok(())
}

#[test]
fn embedded_eas_timezone_drives_recurrence_across_dst() -> anyhow::Result<()> {
    let schedule = crate::runtime::calendar_schedule::prepare(&CalendarScheduleInput::Timed {
        start: "2026-03-23T09:00:00+01:00".into(),
        end: "2026-03-23T10:00:00+01:00".into(),
        time_zone: "Europe/Belgrade".into(),
    })?;
    let mut event = event_at("2026-03-23T08:00:00Z", "2026-03-23T09:00:00Z")?;
    event.fields.time_zone = Patch::Value(schedule.time_zone);
    event.fields.recurrence = recurrence([("type", "0"), ("interval", "1")]);
    let range = range("2026-03-29T00:00:00Z", "2026-03-30T00:00:00Z")?;
    let events = recurrence::expand(event, range, chrono_tz::UTC)?;
    assert_eq!(
        event_times(&events),
        vec![("2026-03-29T07:00:00+00:00".into(), "2026-03-29T08:00:00+00:00".into())]
    );
    Ok(())
}

#[test]
fn moved_and_deleted_exceptions_replace_generated_occurrences() -> anyhow::Result<()> {
    let mut event = event_at("2026-08-03T09:00:00Z", "2026-08-03T10:00:00Z")?;
    event.fields.recurrence = recurrence([("type", "0"), ("interval", "1")]);
    event.fields.exceptions = Patch::Value(vec![
        BTreeMap::from([
            ("exceptionstarttime".into(), "20260810T090000Z".into()),
            ("starttime".into(), "20260810T110000Z".into()),
            ("endtime".into(), "20260810T120000Z".into()),
            ("subject".into(), "Moved".into()),
        ]),
        BTreeMap::from([
            ("exceptionstarttime".into(), "20260811T090000Z".into()),
            ("deleted".into(), "1".into()),
        ]),
    ]);
    let range = range("2026-08-10T00:00:00Z", "2026-08-12T00:00:00Z")?;
    let events = recurrence::expand(event, range, chrono_tz::UTC)?;
    assert_eq!(
        event_times(&events),
        vec![("2026-08-10T11:00:00+00:00".into(), "2026-08-10T12:00:00+00:00".into())]
    );
    let moved = events.first().ok_or_else(|| anyhow::anyhow!("moved occurrence is missing"))?;
    assert_eq!(subject(moved), "Moved");
    Ok(())
}

#[test]
fn monthly_and_yearly_patterns_are_materialized_in_rust() -> anyhow::Result<()> {
    let cases = [
        ([("type", "2"), ("dayofmonth", "15"), ("monthofyear", "0")], "2026-09-15T09:00:00+00:00"),
        ([("type", "3"), ("weekofmonth", "2"), ("dayofweek", "2")], "2026-09-14T09:00:00+00:00"),
        ([("type", "5"), ("dayofmonth", "3"), ("monthofyear", "8")], "2027-08-03T09:00:00+00:00"),
    ];
    for (pattern, expected) in cases {
        let mut event = event_at("2026-08-03T09:00:00Z", "2026-08-03T10:00:00Z")?;
        event.fields.recurrence = recurrence(pattern);
        let range = if expected.starts_with("2027") {
            range("2027-08-01T00:00:00Z", "2027-09-01T00:00:00Z")?
        } else {
            range("2026-09-01T00:00:00Z", "2026-10-01T00:00:00Z")?
        };
        let events = recurrence::expand(event, range, chrono_tz::UTC)?;
        assert_eq!(event_times(&events).first().map(|value| value.0.as_str()), Some(expected));
    }
    let mut event = event_at("2026-08-03T09:00:00Z", "2026-08-03T10:00:00Z")?;
    event.fields.recurrence =
        recurrence([("type", "6"), ("weekofmonth", "1"), ("dayofweek", "2"), ("monthofyear", "8")]);
    let events = recurrence::expand(
        event,
        range("2027-08-01T00:00:00Z", "2027-09-01T00:00:00Z")?,
        chrono_tz::UTC,
    )?;
    assert_eq!(
        event_times(&events).first().map(|value| value.0.as_str()),
        Some("2027-08-02T09:00:00+00:00")
    );
    Ok(())
}

#[test]
fn occurrence_count_and_until_bound_open_ended_patterns() -> anyhow::Result<()> {
    for recurrence_fields in [
        recurrence([("type", "0"), ("occurrences", "2")]),
        recurrence([("type", "0"), ("until", "20260804T090000Z")]),
    ] {
        let mut event = event_at("2026-08-03T09:00:00Z", "2026-08-03T10:00:00Z")?;
        event.fields.recurrence = recurrence_fields;
        let range = range("2026-08-05T00:00:00Z", "2026-08-06T00:00:00Z")?;
        assert!(recurrence::expand(event, range, chrono_tz::UTC)?.is_empty());
    }
    Ok(())
}

fn input(
    query: Option<&str>,
    date_from: Option<&str>,
    date_to: Option<&str>,
    time_zone: Option<&str>,
) -> CalendarSearchInput {
    CalendarSearchInput {
        query: query.map(str::to_owned),
        date_from: date_from.map(str::to_owned),
        date_to: date_to.map(str::to_owned),
        time_zone: time_zone.map(str::to_owned),
        account_ids: None,
        limit: None,
    }
}

fn event_at(start: &str, end: &str) -> anyhow::Result<BackendEvent> {
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

fn recurrence<const N: usize>(values: [(&str, &str); N]) -> Patch<BTreeMap<String, String>> {
    Patch::Value(values.into_iter().map(|(key, value)| (key.into(), value.into())).collect())
}

fn range(start: &str, end: &str) -> anyhow::Result<AgendaRange> {
    Ok(AgendaRange { start: start.parse()?, end: end.parse()? })
}

fn event_times(events: &[BackendEvent]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|event| match (&event.fields.starts_at, &event.fields.ends_at) {
            (Patch::Value(Some(start)), Patch::Value(Some(end))) => {
                Some((start.to_rfc3339(), end.to_rfc3339()))
            }
            _ => None,
        })
        .collect()
}

fn subject(event: &BackendEvent) -> &str {
    match &event.fields.subject {
        Patch::Value(value) => value,
        Patch::Missing => "",
    }
}
