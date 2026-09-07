use std::sync::Arc;

use eas_mail_mcp::{
    CalendarCreateInput, CalendarOperationState, ErrorCode, MailSendInput, OperationJournal,
    OperationState, OperationStatus, RandomIds, Runtime, SystemClock,
};
use eas_mail_mcp_harness::{FakeBackend, MemoryJournal};

#[path = "journal_failures/purge.rs"]
mod purge;

fn runtime(
    backend: Arc<FakeBackend>,
    journal: Arc<MemoryJournal>,
) -> anyhow::Result<(Runtime, tempfile::TempDir)> {
    let directory = tempfile::tempdir()?;
    let runtime = Runtime::with_dependencies(
        vec![backend],
        journal,
        Arc::new(SystemClock),
        Arc::new(RandomIds),
        vec![7; 32],
        directory.path().join("attachments"),
    )?;
    Ok((runtime, directory))
}

fn send_input() -> anyhow::Result<MailSendInput> {
    Ok(serde_json::from_value(serde_json::json!({
        "account_id":"work", "to":["self@example.invalid"], "subject":"s", "body":"b",
        "idempotency_key":"00000000-0000-4000-8000-000000000001"
    }))?)
}

fn calendar_input() -> anyhow::Result<CalendarCreateInput> {
    Ok(serde_json::from_value(serde_json::json!({
        "account_id":"work", "subject":"Meeting", "schedule":{
            "kind":"timed", "start":"2026-09-15T10:00:00Z", "end":"2026-09-15T11:00:00Z", "time_zone":"UTC"
        }, "attendees":[{"email":"guest@example.invalid", "role":"required"}],
        "idempotency_key":"00000000-0000-4000-8000-000000000002"
    }))?)
}

#[tokio::test]
async fn successful_send_records_one_confirmed_step_without_changing_replay() -> anyhow::Result<()>
{
    let backend = Arc::new(FakeBackend::new("work"));
    let journal = Arc::new(MemoryJournal::default());
    let (runtime, _directory) = runtime(backend.clone(), journal)?;
    let input = send_input()?;
    assert_eq!(
        runtime.mail_send(input.clone()).await.data.map(|r| r.status),
        Some(OperationState::Succeeded)
    );
    let inspected = runtime
        .operation_get(eas_mail_mcp::OperationGetInput {
            operation_id: input.idempotency_key.clone(),
        })
        .data
        .ok_or_else(|| anyhow::anyhow!("missing confirmed operation"))?;
    assert_eq!(inspected.completed_steps, 1);
    assert_eq!(inspected.status, OperationStatus::Succeeded);
    assert_eq!(
        runtime.mail_send(input).await.data.map(|r| r.status),
        Some(OperationState::Succeeded)
    );
    assert_eq!(backend.operations()?, ["mail_send"]);
    Ok(())
}

#[tokio::test]
async fn confirmed_send_with_finish_failure_is_unknown_and_never_repeats() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let journal = Arc::new(MemoryJournal::default());
    journal.set_finish_failure(true);
    let (runtime, _directory) = runtime(backend.clone(), journal.clone())?;
    let input = send_input()?;
    let response = runtime.mail_send(input.clone()).await;
    let error = response.error.ok_or_else(|| anyhow::anyhow!("missing durability error"))?;
    assert_eq!(error.code, ErrorCode::OutcomeUnknown);
    assert_eq!(error.operation_id.as_deref(), Some(input.idempotency_key.as_str()));
    assert!(!error.retryable);
    assert_eq!(backend.operations()?, ["mail_send"]);
    journal.set_finish_failure(false);
    let repeated = runtime.mail_send(input.clone()).await;
    assert_eq!(repeated.data.map(|r| r.status), Some(OperationState::Unknown));
    assert_eq!(backend.operations()?, ["mail_send"]);
    assert_eq!(
        journal.lookup(&input.idempotency_key)?.map(|r| r.status),
        Some(OperationStatus::Pending)
    );
    Ok(())
}

#[tokio::test]
async fn checkpoint_failure_stops_calendar_notifications_and_replay() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let journal = Arc::new(MemoryJournal::default());
    journal.set_checkpoint_failure(true);
    let (runtime, _directory) = runtime(backend.clone(), journal.clone())?;
    let input = calendar_input()?;
    let response = runtime.calendar_create(input.clone()).await;
    let error = response.error.ok_or_else(|| anyhow::anyhow!("missing checkpoint error"))?;
    assert_eq!(error.code, ErrorCode::OutcomeUnknown);
    assert_eq!(error.operation_id.as_deref(), Some(input.idempotency_key.as_str()));
    assert!(!error.retryable);
    assert_eq!(backend.operations()?, ["calendar_create_item"]);
    journal.set_checkpoint_failure(false);
    assert_eq!(
        runtime.calendar_create(input).await.data.map(|r| r.status),
        Some(CalendarOperationState::Unknown)
    );
    assert_eq!(backend.operations()?, ["calendar_create_item"]);
    Ok(())
}

#[tokio::test]
async fn calendar_finish_failure_preserves_checkpoints_and_unknown_classification()
-> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let journal = Arc::new(MemoryJournal::default());
    journal.set_finish_failure(true);
    let (runtime, _directory) = runtime(backend.clone(), journal.clone())?;
    let input = calendar_input()?;
    let response = runtime.calendar_create(input.clone()).await;
    assert_eq!(response.error.map(|e| e.code), Some(ErrorCode::OutcomeUnknown));
    assert_eq!(backend.operations()?, ["calendar_create_item", "calendar_send"]);
    assert!(journal.lookup(&input.idempotency_key)?.is_some_and(|r| r.completed_steps != 0));
    journal.set_finish_failure(false);
    assert_eq!(
        runtime.calendar_create(input).await.data.map(|r| r.status),
        Some(CalendarOperationState::Unknown)
    );
    assert_eq!(backend.operations()?.len(), 2);
    Ok(())
}
