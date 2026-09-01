#[path = "calendar_series/faults.rs"]
mod faults;
mod series_support;

use anyhow::Context as _;
use eas_mail_mcp::{
    CalendarDeleteInput, CalendarGetInput, CalendarOperationState, CalendarUpdateInput, ErrorCode,
    PeopleSearchInput,
};
use eas_mail_mcp_harness::FakeBackend;
use serde_json::json;
use series_support::{agenda, create, data, runtime, uuid};
use std::sync::Arc;

#[tokio::test]
async fn personal_series_moves_one_occurrence_splits_and_cleans_up() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(backend.clone())?;
    let created = data(runtime.calendar_create(create(1)?).await)?;
    let master = created.event_ref.context("created reference")?;
    let items = agenda(&runtime).await?;
    assert_eq!(items.len(), 5);
    let occurrence = items.get(1).context("missing occurrence")?.event_ref.clone();
    let moved = data(runtime.calendar_update(serde_json::from_value::<CalendarUpdateInput>(json!({
        "event_ref": occurrence, "scope":"occurrence", "idempotency_key":uuid(2),
        "subject":"Moved occurrence", "schedule":{"kind":"timed","start":"2026-08-25T11:00:00Z","end":"2026-08-25T12:00:00Z","time_zone":"UTC"}
    }))?).await)?;
    assert_eq!(moved.status, CalendarOperationState::Succeeded);
    let items = agenda(&runtime).await?;
    assert_eq!(items.get(1).context("missing occurrence")?.event_ref, occurrence);
    assert_eq!(
        items.get(1).context("missing occurrence")?.starts_at.as_deref(),
        Some("2026-08-25T11:00:00+00:00")
    );
    let detail = data(
        runtime.calendar_get(CalendarGetInput { event_ref: occurrence, body_limit: None }).await,
    )?;
    assert_eq!(detail.subject, "Moved occurrence");
    let split = data(runtime.calendar_update(serde_json::from_value::<CalendarUpdateInput>(json!({
        "event_ref":items.get(2).context("missing occurrence")?.event_ref, "scope":"following", "subject":"New series", "idempotency_key":uuid(3)
    }))?).await)?;
    assert_eq!(split.completed_steps, ["new_series", "truncate_old_series"]);
    let tail = split.event_ref.context("new series reference")?;
    let items = agenda(&runtime).await?;
    assert_eq!(items.len(), 5);
    assert_eq!(items.get(1).context("missing occurrence")?.subject, "Moved occurrence");
    assert_eq!(items.get(2).context("missing occurrence")?.subject, "New series");
    for (reference, key) in [(master, 4), (tail, 5)] {
        let result = data(
            runtime
                .calendar_delete(serde_json::from_value::<CalendarDeleteInput>(json!({
                    "event_ref":reference,"scope":"series","idempotency_key":uuid(key)
                }))?)
                .await,
        )?;
        assert_eq!(result.status, CalendarOperationState::Succeeded);
    }
    assert!(agenda(&runtime).await?.is_empty());
    assert!(!backend.operations()?.iter().any(|name| name == "calendar_send"));
    Ok(())
}

#[tokio::test]
async fn split_failure_checkpoints_and_replay_never_repeat_create() -> anyhow::Result<()> {
    for code in [ErrorCode::ProtocolError, ErrorCode::OutcomeUnknown] {
        let backend = Arc::new(FakeBackend::new("work"));
        let (runtime, _directory) = runtime(backend.clone())?;
        data(runtime.calendar_create(create(10)?).await)?;
        let items = agenda(&runtime).await?;
        backend.set_operation_failure(Some("calendar_update_item"), code)?;
        let input: CalendarUpdateInput = serde_json::from_value(json!({
            "event_ref": items.get(2).context("missing occurrence")?.event_ref, "scope":"following", "subject":"Tail", "idempotency_key":uuid(11)
        }))?;
        let first = data(runtime.calendar_update(input.clone()).await)?;
        assert_eq!(first.completed_steps, ["new_series"]);
        assert_eq!(
            first.status,
            if code == ErrorCode::OutcomeUnknown {
                CalendarOperationState::Unknown
            } else {
                CalendarOperationState::Partial
            }
        );
        let before = backend.operations()?;
        let second = data(runtime.calendar_update(input).await)?;
        assert_eq!(first.status, second.status);
        assert_eq!(before, backend.operations()?);
    }
    Ok(())
}

#[tokio::test]
async fn references_survive_a_new_runtime_and_scope_is_required() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (first, first_dir) = runtime(backend.clone())?;
    data(first.calendar_create(create(20)?).await)?;
    let items = agenda(&first).await?;
    let reference = items.get(2).context("missing occurrence")?.event_ref.clone();
    drop(first);
    drop(first_dir);
    let (second, _directory) = runtime(backend.clone())?;
    let event = data(
        second
            .calendar_get(CalendarGetInput { event_ref: reference.clone(), body_limit: None })
            .await,
    )?;
    assert_eq!(event.starts_at.as_deref(), Some("2026-08-26T10:00:00+00:00"));
    let before = backend.operations()?;
    let rejected = second
        .calendar_delete(serde_json::from_value::<CalendarDeleteInput>(json!({
            "event_ref":reference, "idempotency_key":uuid(21)
        }))?)
        .await;
    assert_eq!(rejected.error.map(|error| error.code), Some(ErrorCode::ValidationFailed));
    assert_eq!(before, backend.operations()?);
    let deleted = data(
        second
            .calendar_delete(serde_json::from_value::<CalendarDeleteInput>(json!({
                "event_ref":reference, "scope":"following", "idempotency_key":uuid(22)
            }))?)
            .await,
    )?;
    assert_eq!(deleted.completed_steps, ["truncate_old_series"]);
    assert_eq!(agenda(&second).await?.len(), 2);
    assert_eq!(
        second
            .calendar_get(CalendarGetInput { event_ref: reference, body_limit: None })
            .await
            .error
            .map(|error| error.code),
        Some(ErrorCode::SyncStale)
    );
    Ok(())
}

#[tokio::test]
async fn directory_search_is_small_and_validated() -> anyhow::Result<()> {
    let (runtime, _directory) = runtime(Arc::new(FakeBackend::new("work")))?;
    let found = data(
        runtime
            .people_search(PeopleSearchInput {
                account_id: None,
                query: "Test".into(),
                limit: Some(1),
            })
            .await,
    )?;
    assert!(found.results_truncated && found.untrusted_external_content);
    assert_eq!(found.items.len(), 1);
    assert_eq!(
        serde_json::to_value(found.items.first().context("missing occurrence")?)?
            .as_object()
            .context("person")?
            .len(),
        2
    );
    assert_eq!(
        runtime
            .people_search(PeopleSearchInput { account_id: None, query: " ".into(), limit: None })
            .await
            .error
            .map(|error| error.code),
        Some(ErrorCode::ValidationFailed)
    );
    Ok(())
}
