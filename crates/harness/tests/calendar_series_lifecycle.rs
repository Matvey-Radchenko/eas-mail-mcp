mod series_support;

use anyhow::Context as _;
use eas_mail_mcp::backend::AccountBackend as _;
use eas_mail_mcp::{
    CalendarAttendeeInput, CalendarAttendeeRole, CalendarGetInput, CalendarOperationState,
    ErrorCode,
};
use eas_mail_mcp_harness::FakeBackend;
use eas_mail_protocol::Patch;
use serde_json::json;
use series_support::{agenda, create, data, runtime, uuid};
use std::sync::Arc;

#[tokio::test]
async fn personal_occurrence_deletion_and_first_following_keep_scope_rules() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(backend.clone())?;
    data(runtime.calendar_create(create(1)?).await)?;
    let items = agenda(&runtime).await?;
    data(
        runtime
            .calendar_delete(serde_json::from_value(json!({
                "event_ref":items.get(2).context("third")?.event_ref,
                "scope":"occurrence", "idempotency_key":uuid(2)
            }))?)
            .await,
    )?;
    assert_eq!(agenda(&runtime).await?.len(), 4);
    let result = data(
        runtime
            .calendar_delete(serde_json::from_value(json!({
                "event_ref":items.first().context("first")?.event_ref,
                "scope":"following", "idempotency_key":uuid(3)
            }))?)
            .await,
    )?;
    assert_eq!(result.completed_steps, ["calendar_item"]);
    assert!(agenda(&runtime).await?.is_empty());
    assert!(backend.calendar_messages()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn meeting_scope_changes_emit_notifications_after_items() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(backend.clone())?;
    let mut input = create(10)?;
    input.attendees.push(guest());
    let master = data(runtime.calendar_create(input).await)?.event_ref.context("master")?;
    let occurrence = agenda(&runtime).await?.get(2).context("third")?.event_ref.clone();
    data(runtime.calendar_update(serde_json::from_value(json!({
        "event_ref":master, "scope":"series", "subject":"Whole series", "idempotency_key":uuid(11)
    }))?).await)?;
    data(runtime.calendar_update(serde_json::from_value(json!({
        "event_ref":occurrence, "scope":"occurrence", "subject":"One instance", "idempotency_key":uuid(12)
    }))?).await)?;
    let cancelled = data(runtime.calendar_cancel(serde_json::from_value(json!({
        "event_ref":occurrence, "scope":"following", "comment":"Tail cancelled", "idempotency_key":uuid(13)
    }))?).await)?;
    assert_eq!(cancelled.completed_steps, ["truncate_old_series", "notify_old_series"]);
    assert_eq!(agenda(&runtime).await?.len(), 2);
    data(
        runtime
            .calendar_cancel(serde_json::from_value(json!({
                "event_ref":master, "scope":"series", "idempotency_key":uuid(14)
            }))?)
            .await,
    )?;
    assert!(agenda(&runtime).await?.is_empty());
    let operations = backend.operations()?;
    assert!(
        operations
            .chunks_exact(2)
            .all(|pair| pair.get(1).is_some_and(|value| value == "calendar_send"))
    );
    assert_eq!(backend.calendar_messages()?.len(), 5);
    Ok(())
}

#[tokio::test]
async fn series_and_occurrence_responses_send_the_original_instance_id() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(backend.clone())?;
    let mut input = create(20)?;
    input.attendees.push(guest());
    let master = data(runtime.calendar_create(input).await)?.event_ref.context("master")?;
    let mut source = backend
        .scan_calendar_metadata()
        .await?
        .events
        .into_iter()
        .find(|event| event.server_id.as_deref() == Some("event-created"))
        .context("created source")?;
    source.fields.organizer_email = Patch::Value("organizer@example.invalid".into());
    source.fields.meeting_status = Patch::Value(3);
    backend.put_calendar_fixture(source)?;
    let instance = agenda(&runtime).await?.get(1).context("second")?.event_ref.clone();
    let before = backend.operations()?;
    let rejected = runtime.calendar_respond(serde_json::from_value(json!({
        "event_ref":instance, "scope":"following", "response":"accept", "idempotency_key":uuid(21)
    }))?).await;
    assert_eq!(rejected.error.map(|value| value.code), Some(ErrorCode::ValidationFailed));
    assert_eq!(backend.operations()?, before);
    for (index, scope, response, reference) in [
        (22, "series", "accept", master),
        (23, "occurrence", "tentative", instance.clone()),
        (24, "occurrence", "decline", instance),
    ] {
        let result = data(runtime.calendar_respond(serde_json::from_value(json!({
            "event_ref":reference,"scope":scope,"response":response,"idempotency_key":uuid(index)
        }))?).await)?;
        assert_eq!(result.status, CalendarOperationState::Succeeded);
    }
    let responses = backend.calendar_responses()?;
    assert_eq!(responses.first(), Some(&None));
    assert_eq!(responses.get(1), responses.get(2));
    assert!(responses.get(1).is_some_and(Option::is_some));
    Ok(())
}

#[tokio::test]
async fn concurrent_split_replays_once_and_keeps_distinct_uids() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(backend.clone())?;
    let master = data(runtime.calendar_create(create(30)?).await)?.event_ref.context("master")?;
    let reference = agenda(&runtime).await?.get(2).context("third")?.event_ref.clone();
    let input = serde_json::from_value::<eas_mail_mcp::CalendarUpdateInput>(json!({
        "event_ref":reference, "scope":"following", "subject":"Tail", "idempotency_key":uuid(31)
    }))?;
    let (first, second) =
        tokio::join!(runtime.calendar_update(input.clone()), runtime.calendar_update(input));
    let first = data(first)?;
    let second = data(second)?;
    assert_eq!(first.status, second.status);
    // Either call can win the lock; replay does not persist content-bearing references.
    let tail = first.event_ref.or(second.event_ref).context("tail")?;
    let original =
        data(runtime.calendar_get(CalendarGetInput { event_ref: master, body_limit: None }).await)?;
    let new =
        data(runtime.calendar_get(CalendarGetInput { event_ref: tail, body_limit: None }).await)?;
    assert_ne!(original.uid, new.uid);
    assert_eq!(
        backend.operations()?.iter().filter(|value| *value == "calendar_create_item").count(),
        2
    );
    assert_eq!(agenda(&runtime).await?.len(), 5);
    Ok(())
}

#[tokio::test]
async fn participants_on_one_exception_keep_organizer_rights_and_receive_tail_cancellation()
-> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(backend.clone())?;
    let master = data(runtime.calendar_create(create(50)?).await)?.event_ref.context("master")?;
    let selected = agenda(&runtime).await?.get(2).context("third")?.event_ref.clone();
    data(
        runtime
            .calendar_update(serde_json::from_value(json!({
                "event_ref":selected, "scope":"occurrence", "attendees":[guest()],
                "idempotency_key":uuid(51)
            }))?)
            .await,
    )?;
    let current = data(
        runtime
            .calendar_get(CalendarGetInput { event_ref: master.clone(), body_limit: None })
            .await,
    )?;
    assert!(current.can_update && current.can_cancel && !current.can_delete);
    data(runtime.calendar_update(serde_json::from_value(json!({
        "event_ref":master, "scope":"series", "subject":"Still my series", "idempotency_key":uuid(52)
    }))?).await)?;
    let before = backend.calendar_messages()?.len();
    let result = data(
        runtime
            .calendar_cancel(serde_json::from_value(json!({
                "event_ref":selected, "scope":"following", "idempotency_key":uuid(53)
            }))?)
            .await,
    )?;
    assert!(result.completed_steps.contains(&"notify_removed_attendees".into()));
    assert_eq!(backend.calendar_messages()?.len(), before + 1);
    assert_eq!(agenda(&runtime).await?.len(), 2);
    Ok(())
}

fn guest() -> CalendarAttendeeInput {
    CalendarAttendeeInput {
        email: "guest@example.invalid".into(),
        name: None,
        role: CalendarAttendeeRole::Required,
    }
}
