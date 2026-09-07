#[expect(dead_code, reason = "shared integration-test support is compiled once per test binary")]
mod support;

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use eas_mail_mcp::backend::AccountBackend;
use eas_mail_mcp::{CalendarFindRecurringSlotsInput, ErrorCode, Runtime};
use eas_mail_mcp_harness::{ExpectedCall, FixedClock, MemoryJournal, SequenceIds};
use eas_mail_protocol::protocol::build_availability;
use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{Command, RequestSafety};

#[tokio::test]
async fn weekly_uses_one_padded_request_per_date_without_sync_and_keeps_partial_data()
-> anyhow::Result<()> {
    let input = input()?;
    let calls = vec![
        support::options_with_calendar(),
        page_call(3, 200, response(Some("00000"))?)?,
        page_call(10, 200, response(None)?)?,
        page_call(17, 200, response(Some("00000"))?)?,
    ];
    let (runtime, transport, _directory) = runtime(calls)?;
    let result = runtime.calendar_find_recurring_slots(input).await;
    let data = result.data.ok_or_else(|| anyhow::anyhow!("no data: {:?}", result.error))?;
    let pattern = data.suggestions.first().ok_or_else(|| anyhow::anyhow!("no pattern"))?;
    assert_eq!(pattern.required_available_occurrences, 2);
    assert_eq!(pattern.occurrences.len(), 3);
    assert_eq!(
        pattern
            .occurrences
            .get(1)
            .ok_or_else(|| anyhow::anyhow!("missing fixture result"))?
            .conflicts
            .len(),
        1
    );
    assert_eq!(
        pattern
            .occurrences
            .get(1)
            .ok_or_else(|| anyhow::anyhow!("missing fixture result"))?
            .starts_at,
        "2026-08-10T09:00:00+00:00"
    );
    assert!(
        data.participants
            .first()
            .ok_or_else(|| anyhow::anyhow!("missing fixture result"))?
            .has_no_data
    );
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn throttled_week_stops_remaining_requests_and_marks_them_unknown() -> anyhow::Result<()> {
    let calls = vec![
        support::options_with_calendar(),
        page_call(3, 200, response(Some("00000"))?)?,
        page_call(10, 429, Vec::new())?,
    ];
    let (runtime, transport, _directory) = runtime(calls)?;
    let result = runtime.calendar_find_recurring_slots(input()?).await;
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(
        result.warnings.first().ok_or_else(|| anyhow::anyhow!("missing fixture result"))?.code,
        "THROTTLED"
    );
    let data = result.data.ok_or_else(|| anyhow::anyhow!("partial data missing"))?;
    assert_eq!(
        data.suggestions
            .first()
            .ok_or_else(|| anyhow::anyhow!("missing fixture result"))?
            .required_available_occurrences,
        1
    );
    assert_eq!(
        data.suggestions
            .first()
            .ok_or_else(|| anyhow::anyhow!("missing fixture result"))?
            .occurrences
            .get(2)
            .ok_or_else(|| anyhow::anyhow!("missing fixture result"))?
            .conflicts
            .len(),
        1
    );
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn no_successful_availability_request_returns_failure_not_empty_identity_results()
-> anyhow::Result<()> {
    let (runtime, transport, _directory) =
        runtime(vec![support::options_with_calendar(), page_call(3, 429, Vec::new())?])?;
    let result = runtime.calendar_find_recurring_slots(input()?).await;
    assert!(result.data.is_none());
    assert_eq!(result.error.map(|error| error.code), Some(ErrorCode::Throttled));
    transport.verify_complete()?;
    Ok(())
}

fn input() -> anyhow::Result<CalendarFindRecurringSlotsInput> {
    Ok(serde_json::from_value(serde_json::json!({
        "account_id":"work", "participants":["person@example.invalid"],
        "date_from":"2026-08-03", "date_to":"2026-08-17", "time_zone":"UTC",
        "working_hours":[{"weekdays":["mon"], "start":"09:00", "end":"11:00"}],
        "duration_minutes":30, "buffer_minutes":15, "weekday":"mon", "limit":1
    }))?)
}

fn page_call(day: u32, status: u16, response: Vec<u8>) -> anyhow::Result<ExpectedCall> {
    let start = format!("2026-08-{day:02}T08:45:00Z").parse::<DateTime<Utc>>()?;
    Ok(support::call(
        Command::ResolveRecipients,
        build_availability(
            &["person@example.invalid".into()],
            start,
            start + Duration::minutes(150),
        )?,
        Some(123),
        RequestSafety::RetrySafe,
        status,
        response,
    ))
}

fn response(slots: Option<&str>) -> anyhow::Result<Vec<u8>> {
    let mut root = Element::new("ResolveRecipients", "ResolveRecipients");
    root.push(Element::text("ResolveRecipients", "Status", "1"));
    let mut response = Element::new("ResolveRecipients", "Response");
    response.push(Element::text("ResolveRecipients", "To", "person@example.invalid"));
    response.push(Element::text("ResolveRecipients", "Status", "1"));
    response.push(Element::text("ResolveRecipients", "RecipientCount", "1"));
    let mut recipient = Element::new("ResolveRecipients", "Recipient");
    recipient.push(Element::text("ResolveRecipients", "Type", "1"));
    recipient.push(Element::text("ResolveRecipients", "DisplayName", "Person"));
    recipient.push(Element::text("ResolveRecipients", "EmailAddress", "person@example.invalid"));
    let mut availability = Element::new("ResolveRecipients", "Availability");
    availability.push(Element::text(
        "ResolveRecipients",
        "Status",
        if slots.is_some() { "1" } else { "162" },
    ));
    if let Some(slots) = slots {
        availability.push(Element::text("ResolveRecipients", "MergedFreeBusy", slots));
    }
    recipient.push(availability);
    response.push(recipient);
    root.push(response);
    Ok(encode(&root)?)
}

fn runtime(
    calls: Vec<ExpectedCall>,
) -> anyhow::Result<(Runtime, Arc<eas_mail_mcp_harness::ScriptedTransport>, tempfile::TempDir)> {
    let (mailbox, transport) = support::mailbox(calls, support::default_policy())?;
    let directory = tempfile::tempdir()?;
    let backend: Arc<dyn AccountBackend> = Arc::new(mailbox);
    let runtime = Runtime::with_dependencies(
        vec![backend],
        Arc::new(MemoryJournal::default()),
        Arc::new(FixedClock::new(DateTime::UNIX_EPOCH)),
        Arc::new(SequenceIds::default()),
        vec![7; 32],
        directory.path().join("attachments"),
    )?;
    Ok((runtime, transport, directory))
}
