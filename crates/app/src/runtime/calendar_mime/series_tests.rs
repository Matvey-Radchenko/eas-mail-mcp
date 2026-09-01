use anyhow::Context as _;
use chrono::{DateTime, Duration, Utc};
use eas_mail_protocol::{CalendarException, CalendarFields, Patch};
use serde_json::json;

use super::*;
use crate::runtime::{calendar_prepare, calendar_series};

#[test]
fn recurring_invitation_keeps_dst_and_original_exception_identity() -> anyhow::Result<()> {
    let mut event = series(false)?;
    let original = instant("2026-03-08T13:00:00Z")?;
    event.properties.exceptions.push(CalendarException {
        original_start: original,
        deleted: false,
        fields: CalendarFields {
            subject: Patch::Value("Moved; one, occurrence".into()),
            starts_at: Patch::Value(Some(original + Duration::hours(1))),
            ends_at: Patch::Value(Some(original + Duration::hours(2))),
            ..Default::default()
        },
    });
    event.properties.exceptions.push(CalendarException {
        original_start: instant("2026-03-15T13:00:00Z")?,
        deleted: true,
        fields: CalendarFields::default(),
    });
    let rendered = calendar(
        "owner@example.invalid",
        &event.attendees,
        &event,
        None,
        CalendarMessageMethod::Request,
    )?
    .replace("\r\n ", "");
    assert_eq!(rendered.matches("BEGIN:VEVENT").count(), 2);
    assert!(rendered.contains("RRULE:FREQ=WEEKLY;BYDAY=SU;INTERVAL=1;WKST=MO;COUNT=4"));
    assert!(rendered.contains("BEGIN:VTIMEZONE") && rendered.contains("BEGIN:DAYLIGHT"));
    assert!(rendered.contains("TZOFFSETFROM:-0500") && rendered.contains("TZOFFSETTO:-0400"));
    assert!(rendered.contains("RECURRENCE-ID;TZID=EAS-") && rendered.contains(":20260308T090000"));
    assert!(rendered.contains(":20260308T100000"));
    assert!(rendered.contains("EXDATE;TZID=EAS-") && rendered.contains(":20260315T090000"));
    assert!(rendered.contains("SUMMARY:Moved\\; one\\, occurrence"));
    assert!(!rendered.contains("THISANDFUTURE"));
    Ok(())
}

#[test]
fn occurrence_reply_and_cancel_never_include_a_new_recurrence_rule() -> anyhow::Result<()> {
    let master = series(false)?;
    let original = instant("2026-03-08T13:00:00Z")?;
    let occurrence = calendar_series::selected(&master, original)?;
    for method in [
        CalendarMessageMethod::Cancel,
        CalendarMessageMethod::Reply(CalendarResponseChoice::Tentative),
    ] {
        let rendered =
            calendar("owner@example.invalid", &master.attendees, &occurrence, None, method)?;
        assert!(rendered.contains("RECURRENCE-ID"));
        assert_eq!(rendered.matches("BEGIN:VEVENT").count(), 1);
        let event = rendered
            .split("BEGIN:VEVENT")
            .nth(1)
            .context("event")?
            .split("END:VEVENT")
            .next()
            .context("event end")?;
        assert!(!event.contains("RRULE:"));
    }
    Ok(())
}

#[test]
fn all_day_rule_and_changed_value_type_keep_date_recurrence_id() -> anyhow::Result<()> {
    let master = series(true)?;
    let first = calendar_series::prepared(master.clone())?;
    let output = calendar(
        "owner@example.invalid",
        &master.attendees,
        &master,
        first.all_day_dates,
        CalendarMessageMethod::Request,
    )?;
    assert!(output.contains("DTSTART;VALUE=DATE:20260301"));
    assert!(!output.contains("BEGIN:VTIMEZONE"));
    let mut occurrence = calendar_series::selected(&master, instant("2026-03-08T05:00:00Z")?)?;
    occurrence.all_day = false;
    occurrence.starts_at = instant("2026-03-08T13:00:00Z")?;
    occurrence.ends_at = instant("2026-03-08T14:00:00Z")?;
    let output = calendar(
        "owner@example.invalid",
        &master.attendees,
        &occurrence,
        None,
        CalendarMessageMethod::Request,
    )?;
    assert!(output.contains("RECURRENCE-ID;VALUE=DATE:20260308"));
    assert!(output.contains("BEGIN:VTIMEZONE"));
    Ok(())
}

#[test]
fn all_day_master_with_timed_exception_includes_its_timezone() -> anyhow::Result<()> {
    let mut item = series(true)?;
    let dates = calendar_series::prepared(item.clone())?.all_day_dates;
    item.properties.exceptions.push(CalendarException {
        original_start: instant("2026-03-08T05:00:00Z")?,
        deleted: false,
        fields: CalendarFields {
            all_day: Patch::Value(false),
            starts_at: Patch::Value(Some(instant("2026-03-08T13:00:00Z")?)),
            ends_at: Patch::Value(Some(instant("2026-03-08T14:00:00Z")?)),
            ..Default::default()
        },
    });
    let output = calendar(
        "owner@example.invalid",
        &item.attendees,
        &item,
        dates,
        CalendarMessageMethod::Request,
    )?
    .replace("\r\n ", "");
    assert_eq!(output.matches("BEGIN:VTIMEZONE").count(), 1);
    assert_eq!(output.matches("BEGIN:VEVENT").count(), 2);
    assert!(output.contains("RECURRENCE-ID;VALUE=DATE:20260308"));
    assert!(output.contains("DTSTART;TZID=EAS-") && output.contains(":20260308T090000"));
    Ok(())
}

#[test]
fn exception_attendees_do_not_replace_the_master_attendee_list() -> anyhow::Result<()> {
    let mut item = series(false)?;
    let second = CalendarAttendee {
        email: "second@example.invalid".into(),
        name: "Second".into(),
        attendee_type: 2,
        attendee_status: 0,
    };
    item.properties.exceptions.push(CalendarException {
        original_start: instant("2026-03-08T13:00:00Z")?,
        deleted: false,
        fields: CalendarFields {
            attendees: Patch::Value(vec![second.clone()]),
            ..Default::default()
        },
    });
    let envelope = [item.attendees.first().context("master attendee")?.clone(), second];
    let output =
        calendar("owner@example.invalid", &envelope, &item, None, CalendarMessageMethod::Request)?;
    let output = output.replace("\r\n ", "");
    let components: Vec<_> = output.split("BEGIN:VEVENT").skip(1).collect();
    assert!(components.first().context("master")?.contains("guest@example.invalid"));
    assert!(!components.first().context("master")?.contains("second@example.invalid"));
    assert!(components.get(1).context("exception")?.contains("second@example.invalid"));
    Ok(())
}

#[test]
fn icalendar_month_end_rules_match_exchange_instead_of_skipping_short_months() -> anyhow::Result<()>
{
    use eas_mail_protocol::{CalendarRecurrence, RecurrenceEnd, RecurrencePattern};

    for (pattern, expected) in [
        (RecurrencePattern::Monthly { day: 31 }, "FREQ=MONTHLY;BYMONTHDAY=-1"),
        (RecurrencePattern::Monthly { day: 30 }, "FREQ=MONTHLY;BYMONTHDAY=28,29,30;BYSETPOS=-1"),
        (
            RecurrencePattern::Yearly { month: 2, day: 29 },
            "FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=28,29;BYSETPOS=-1",
        ),
        (RecurrencePattern::MonthlyRelative { days: 127, week: 5 }, "FREQ=MONTHLY;BYMONTHDAY=-1"),
    ] {
        let mut item = series(false)?;
        item.properties.recurrence = Some(CalendarRecurrence {
            pattern,
            interval: 1,
            first_day_of_week: 1,
            end: RecurrenceEnd::Count(3),
        });
        let output = calendar(
            "owner@example.invalid",
            &item.attendees,
            &item,
            None,
            CalendarMessageMethod::Request,
        )?
        .replace("\r\n ", "");
        assert!(output.contains(&format!("RRULE:{expected};INTERVAL=1;WKST=MO;COUNT=3")));
    }
    Ok(())
}

fn series(all_day: bool) -> anyhow::Result<CalendarApplication> {
    let schedule = if all_day {
        json!({"kind":"all_day", "start_date":"2026-03-01", "end_date":"2026-03-02", "time_zone":"America/New_York"})
    } else {
        json!({"kind":"timed", "start":"2026-03-01T09:00:00-05:00", "end":"2026-03-01T10:00:00-05:00", "time_zone":"America/New_York"})
    };
    let input = serde_json::from_value(json!({
        "account_id":"work", "subject":"Series", "schedule":schedule,
        "attendees":[{"email":"guest@example.invalid", "role":"required"}],
        "recurrence":{"frequency":"weekly", "end":{"mode":"count", "count":4}},
        "idempotency_key":"11111111-2222-4333-8444-555555555555"
    }))?;
    Ok(calendar_prepare::create(
        &input,
        DateTime::UNIX_EPOCH,
        "uid".into(),
        "owner@example.invalid",
    )?
    .mutation
    .application)
}

fn instant(value: &str) -> anyhow::Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc))
}
