mod series_support;

use anyhow::Context as _;
use eas_mail_mcp::backend::AccountBackend as _;
use eas_mail_mcp::{CalendarAttendeeInput, CalendarAttendeeRole, ErrorCode};
use eas_mail_mcp_harness::FakeBackend;
use eas_mail_protocol::{CalendarException, CalendarFields, Patch};
use serde_json::json;
use series_support::{agenda, create, data, runtime, uuid};
use std::sync::Arc;

fn guest(index: usize) -> CalendarAttendeeInput {
    CalendarAttendeeInput {
        email: format!("guest{index}@example.invalid"),
        name: None,
        role: CalendarAttendeeRole::Required,
    }
}

#[tokio::test]
async fn content_schedule_and_role_changes_notify_existing_attendees() -> anyhow::Result<()> {
    for (index, change) in [
        json!({"location":"New room"}),
        json!({"subject":"New subject"}),
        json!({"body":"New agenda"}),
        json!({"schedule":{"kind":"timed","start":"2026-08-24T12:00:00Z",
            "end":"2026-08-24T13:00:00Z","time_zone":"UTC"}}),
        json!({"recurrence":{"frequency":"daily","end":{"mode":"count","count":4}}}),
        json!({"attendees":[{"email":"guest0@example.invalid","role":"optional"}]}),
    ]
    .into_iter()
    .enumerate()
    {
        for recurring in [false, true] {
            if !recurring && change.get("recurrence").is_some() {
                continue;
            }
            let backend = Arc::new(FakeBackend::new("work"));
            let (runtime, _directory) = runtime(backend.clone())?;
            let mut input = create(80)?;
            if !recurring {
                input.recurrence = None;
            }
            input.attendees = vec![guest(0)];
            let reference =
                data(runtime.calendar_create(input).await)?.event_ref.context("master")?;
            let before = backend.calendar_messages()?.len();
            let mut update = change.clone();
            let fields = update.as_object_mut().context("update fields")?;
            fields.insert("event_ref".into(), json!(reference));
            fields.insert("scope".into(), json!("series"));
            fields.insert("idempotency_key".into(), json!(uuid(81)));
            data(runtime.calendar_update(serde_json::from_value(update)?).await)?;
            assert_eq!(
                backend.calendar_messages()?.len(),
                before + 1,
                "change {index}, recurring {recurring}"
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn online_series_adds_three_people_without_reinviting_37_or_rewriting_seven_exceptions()
-> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(backend.clone())?;
    let mut input = create(82)?;
    input.recurrence = Some(serde_json::from_value(json!({
        "frequency":"weekly","interval":2,"weekdays":["mon"],
        "end":{"mode":"count","count":13}
    }))?);
    input.attendees = (0..37).map(guest).collect();
    let master = data(runtime.calendar_create(input).await)?.event_ref.context("master")?;
    let mut source = backend
        .scan_calendar_metadata()
        .await?
        .events
        .into_iter()
        .find(|event| event.server_id.as_deref() == Some("event-created"))
        .context("source")?;
    let properties = source.fields.properties.as_mut().context("properties")?;
    properties.online_meeting_conf_link = Some("conf://example.invalid/meeting".into());
    properties.online_meeting_external_link = Some("https://example.invalid/meeting".into());
    properties.appointment_reply_time = Some(chrono::DateTime::UNIX_EPOCH);
    let start = chrono::DateTime::parse_from_rfc3339("2026-08-24T10:00:00Z")?.to_utc();
    properties.exceptions = (1..=7)
        .map(|index| CalendarException {
            original_start: start + chrono::Duration::weeks(index * 2),
            deleted: false,
            fields: CalendarFields {
                location: Patch::Value("Occurrence room".into()),
                reminder_minutes: Patch::Value(None),
                ..Default::default()
            },
        })
        .collect();
    let retained = properties.clone();
    backend.put_calendar_fixture(source)?;
    let before = backend.calendar_messages()?.len();
    let update = serde_json::from_value::<eas_mail_mcp::CalendarUpdateInput>(json!({
        "event_ref":master,"scope":"series","attendees":(0..40).map(guest).collect::<Vec<_>>(),
        "idempotency_key":uuid(83)
    }))?;
    data(runtime.calendar_update(update.clone()).await)?;
    data(runtime.calendar_update(update).await)?;
    let messages = backend.calendar_messages()?;
    assert_eq!(messages.len(), before + 1);
    let message = String::from_utf8(messages.last().context("notification")?.clone())?;
    assert!(message.contains("guest37@example.invalid"));
    assert!(message.contains("guest38@example.invalid"));
    assert!(message.contains("guest39@example.invalid"));
    // MIME may describe all attendees; only the envelope headers choose recipients.
    let headers = message.split("\r\n\r\n").next().context("headers")?;
    assert!(!headers.contains("guest0@example.invalid"));
    let result = backend
        .scan_calendar_metadata()
        .await?
        .events
        .into_iter()
        .find(|event| event.server_id.as_deref() == Some("event-created"))
        .context("result")?;
    assert_eq!(result.fields.properties, Some(retained));
    Ok(())
}

#[tokio::test]
async fn changed_disabled_reminder_fails_before_any_series_mutation() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(backend.clone())?;
    data(runtime.calendar_create(create(84)?).await)?;
    let mut source = backend
        .scan_calendar_metadata()
        .await?
        .events
        .into_iter()
        .find(|event| event.server_id.as_deref() == Some("event-created"))
        .context("source")?;
    let start = chrono::DateTime::parse_from_rfc3339("2026-08-26T10:00:00Z")?.to_utc();
    source.fields.properties.as_mut().context("properties")?.exceptions.push(CalendarException {
        original_start: start,
        deleted: false,
        fields: CalendarFields { reminder_minutes: Patch::Value(None), ..Default::default() },
    });
    backend.put_calendar_fixture(source)?;
    let occurrence = agenda(&runtime).await?.get(2).context("third")?.event_ref.clone();
    let before = backend.operations()?;
    for (index, scope) in [(85, "occurrence"), (86, "following")] {
        let result = runtime.calendar_update(serde_json::from_value(json!({
            "event_ref":occurrence,"scope":scope,"subject":"Changed", "idempotency_key":uuid(index)
        }))?).await;
        assert_eq!(result.error.map(|error| error.code), Some(ErrorCode::ValidationFailed));
        assert_eq!(backend.operations()?, before);
    }
    Ok(())
}
