use chrono::{NaiveDate, TimeZone as _, Utc};

use super::*;

#[test]
fn request_cancel_and_reply_have_rfc5546_shapes() -> anyhow::Result<()> {
    let item = event()?;
    let guest = attendees();
    let request =
        calendar("owner@example.invalid", &guest, &item, None, CalendarMessageMethod::Request)?;
    assert!(request.contains("METHOD:REQUEST"));
    assert!(request.contains("UID:event-uid@example.invalid"));
    assert!(request.contains("ORGANIZER:mailto:owner@example.invalid"));
    assert!(request.contains("ROLE=OPT-PARTICIPANT"));
    assert!(request.contains("PARTSTAT=NEEDS-ACTION"));
    assert!(request.contains("DTSTART:20260824T090000Z"));

    let cancel =
        calendar("owner@example.invalid", &guest, &item, None, CalendarMessageMethod::Cancel)?;
    assert!(cancel.contains("METHOD:CANCEL"));
    assert!(cancel.contains("STATUS:CANCELLED"));

    let organizer = vec![CalendarAttendee {
        email: "organizer@example.invalid".into(),
        name: "Organizer".into(),
        attendee_type: 1,
        attendee_status: 0,
    }];
    let reply = calendar(
        "owner@example.invalid",
        &organizer,
        &item,
        None,
        CalendarMessageMethod::Reply(CalendarResponseChoice::Accept),
    )?;
    assert!(reply.contains("METHOD:REPLY"));
    assert!(reply.contains("ORGANIZER:mailto:organizer@example.invalid"));
    assert!(reply.contains("PARTSTAT=ACCEPTED"));
    assert!(reply.contains("ATTENDEE"));
    let unfolded = reply.replace("\r\n ", "").to_ascii_lowercase();
    assert!(unfolded.contains("mailto:owner@example.invalid"));
    Ok(())
}

#[test]
fn all_day_dates_and_text_are_escaped() -> anyhow::Result<()> {
    let mut item = event()?;
    item.subject = "Planning, phase; one".into();
    item.body = "Line one\nLine two".into();
    let start = NaiveDate::from_ymd_opt(2026, 8, 24)
        .ok_or_else(|| anyhow::anyhow!("invalid fixture date"))?;
    let end = NaiveDate::from_ymd_opt(2026, 8, 26)
        .ok_or_else(|| anyhow::anyhow!("invalid fixture date"))?;
    let output = calendar(
        "owner@example.invalid",
        &attendees(),
        &item,
        Some((start, end)),
        CalendarMessageMethod::Request,
    )?;
    assert!(output.contains("DTSTART;VALUE=DATE:20260824"));
    assert!(output.contains("DTEND;VALUE=DATE:20260826"));
    assert!(output.contains("SUMMARY:Planning\\, phase\\; one"));
    assert!(output.contains("DESCRIPTION:Line one\\nLine two"));
    Ok(())
}

#[test]
fn mime_contains_plain_and_calendar_parts_without_header_injection() -> anyhow::Result<()> {
    let item = event()?;
    let bytes = build(
        "owner@example.invalid",
        &attendees(),
        &item,
        None,
        CalendarMessageMethod::Request,
        "Visible comment",
    )?;
    let output = String::from_utf8(bytes)?;
    assert!(output.contains("Subject: Planning"));
    assert!(output.contains("Content-Type: multipart/alternative"));
    assert!(output.contains("text/calendar"));
    assert!(output.contains("method=REQUEST") || output.contains("method=\"REQUEST\""));
    assert!(output.contains("Content-Class: urn:content-classes:calendarmessage"));
    assert!(output.contains("Content-Transfer-Encoding: base64"));
    assert!(!output.contains("@eas-mail-mcp.local>"));
    assert!(output.contains("Visible comment"));

    let mut bad_subject = item.clone();
    bad_subject.subject = "Injected\r\nBcc: target@example.invalid".into();
    assert!(
        build(
            "owner@example.invalid",
            &attendees(),
            &bad_subject,
            None,
            CalendarMessageMethod::Request,
            "",
        )
        .is_err()
    );
    let mut bad_attendee = attendees();
    bad_attendee.first_mut().ok_or_else(|| anyhow::anyhow!("attendee fixture is empty"))?.name =
        "Guest\nBcc".into();
    assert!(
        build(
            "owner@example.invalid",
            &bad_attendee,
            &item,
            None,
            CalendarMessageMethod::Request,
            "",
        )
        .is_err()
    );
    Ok(())
}

fn event() -> anyhow::Result<CalendarApplication> {
    let starts_at = Utc
        .with_ymd_and_hms(2026, 8, 24, 9, 0, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("invalid fixture instant"))?;
    Ok(CalendarApplication {
        properties: Default::default(),
        time_zone: "AAAA".into(),
        uid: "event-uid@example.invalid".into(),
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
        attendees: attendees(),
    })
}

fn attendees() -> Vec<CalendarAttendee> {
    vec![CalendarAttendee {
        email: "guest@example.invalid".into(),
        name: "Guest".into(),
        attendee_type: 2,
        attendee_status: 0,
    }]
}
