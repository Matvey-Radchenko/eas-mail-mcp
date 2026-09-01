use std::sync::Arc;

use eas_mail_mcp::backend::AccountBackend;
use eas_mail_mcp::{
    CalendarAttendeeInput, CalendarAttendeeRole, CalendarBusyStatus, CalendarCancelInput,
    CalendarCreateInput, CalendarDeleteInput, CalendarMailKind, CalendarOperationState,
    CalendarRespondInput, CalendarResponseChoice, CalendarScheduleInput, CalendarSearchInput,
    CalendarUpdateInput, ErrorCode, MailSearchInput, OperationJournal, OperationStatus, Runtime,
};
use eas_mail_mcp_harness::{FakeBackend, FixedClock, MemoryJournal, SequenceIds};

#[tokio::test]
async fn all_calendar_lifecycle_tools_execute_expected_steps() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory, _) = make_runtime(backend.clone())?;

    let personal = runtime.calendar_create(create_input(false, uuid(1))).await;
    let personal_ref = personal
        .data
        .and_then(|result| result.event_ref)
        .ok_or_else(|| anyhow::anyhow!("personal create returned no event_ref"))?;
    let deleted = runtime
        .calendar_delete(CalendarDeleteInput {
            scope: None,
            event_ref: personal_ref,
            idempotency_key: uuid(2),
        })
        .await;
    assert_eq!(deleted.data.map(|result| result.status), Some(CalendarOperationState::Succeeded));

    let update_ref = event_ref(&runtime, "planning").await?;
    let update = CalendarUpdateInput {
        scope: None,
        recurrence: None,
        event_ref: update_ref,
        subject: Some("Updated planning".into()),
        schedule: None,
        body: Some(String::new()),
        location: None,
        reminder_minutes: None,
        clear_reminder: true,
        busy_status: Some(CalendarBusyStatus::Tentative),
        attendees: Some(Vec::new()),
        idempotency_key: uuid(3),
    };
    let updated = runtime.calendar_update(update.clone()).await;
    assert_eq!(
        updated.data.as_ref().map(|result| result.status),
        Some(CalendarOperationState::Succeeded)
    );
    assert_eq!(
        updated.data.as_ref().map(|result| result.completed_steps.clone()),
        Some(vec!["calendar_item".into(), "notify_removed_attendees".into()])
    );
    let replayed = runtime.calendar_update(update).await;
    assert_eq!(replayed.data.map(|result| result.status), Some(CalendarOperationState::Succeeded));

    let cancel_ref = event_ref(&runtime, "planning").await?;
    let cancelled = runtime
        .calendar_cancel(CalendarCancelInput {
            scope: None,
            event_ref: cancel_ref,
            comment: "Cancelled for testing".into(),
            idempotency_key: uuid(4),
        })
        .await;
    assert_eq!(cancelled.data.map(|result| result.status), Some(CalendarOperationState::Succeeded));

    let respond_ref = event_ref(&runtime, "received").await?;
    let responded = runtime
        .calendar_respond(CalendarRespondInput {
            scope: None,
            event_ref: respond_ref,
            response: CalendarResponseChoice::Accept,
            comment: "Accepted".into(),
            idempotency_key: uuid(5),
        })
        .await;
    assert_eq!(responded.data.map(|result| result.status), Some(CalendarOperationState::Succeeded));

    assert_eq!(
        backend.operations()?,
        [
            "calendar_create_item",
            "calendar_delete_item",
            "calendar_update_item",
            "calendar_send",
            "calendar_delete_item",
            "calendar_send",
            "calendar_respond_item",
            "calendar_send",
        ]
    );
    Ok(())
}

#[tokio::test]
async fn completed_update_replays_with_a_portable_reference_and_detects_conflict()
-> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory, _) = make_runtime(backend.clone())?;
    let input = CalendarUpdateInput {
        scope: None,
        recurrence: None,
        event_ref: event_ref(&runtime, "planning").await?,
        subject: Some("Updated".into()),
        schedule: None,
        body: None,
        location: None,
        reminder_minutes: None,
        clear_reminder: false,
        busy_status: None,
        attendees: None,
        idempotency_key: uuid(10),
    };
    let first = runtime.calendar_update(input.clone()).await;
    let repeated = runtime.calendar_update(input.clone()).await;
    assert_eq!(first.data.map(|result| result.status), Some(CalendarOperationState::Succeeded));
    assert_eq!(repeated.data.map(|result| result.status), Some(CalendarOperationState::Succeeded));
    assert_eq!(backend.operations()?, ["calendar_update_item", "calendar_send"]);

    let mut changed = input;
    changed.subject = Some("Different".into());
    assert_eq!(
        runtime.calendar_update(changed).await.error.map(|error| error.code),
        Some(ErrorCode::IdempotencyConflict)
    );
    Ok(())
}

#[tokio::test]
async fn meeting_request_mail_ref_responds_through_search_long_id() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory, journal) = make_runtime(backend.clone())?;
    let summary = runtime
        .mail_search(MailSearchInput {
            query: "meeting-request".into(),
            account_ids: Some(vec!["work".into()]),
            cursor: None,
            limit: Some(1),
        })
        .await
        .data
        .and_then(|page| page.items.into_iter().next())
        .ok_or_else(|| anyhow::anyhow!("meeting request mail is missing"))?;
    assert_eq!(summary.calendar_message, Some(CalendarMailKind::Request));
    assert!(summary.can_respond);

    let input = CalendarRespondInput {
        scope: None,
        event_ref: summary.mail_ref,
        response: CalendarResponseChoice::Accept,
        comment: "Accepted from Inbox".into(),
        idempotency_key: uuid(11),
    };
    let result = runtime
        .calendar_respond(input.clone())
        .await
        .data
        .ok_or_else(|| anyhow::anyhow!("meeting response result is missing"))?;
    assert_eq!(result.status, CalendarOperationState::Succeeded);
    assert_eq!(result.completed_steps, ["meeting_response", "reply_notification"]);
    assert!(result.event_ref.is_none());
    assert_eq!(backend.operations()?, ["calendar_respond_request", "calendar_send"]);
    assert_eq!(
        journal.lookup(&uuid(11))?.map(|value| value.status),
        Some(OperationStatus::Succeeded)
    );
    assert_eq!(
        runtime.calendar_respond(input.clone()).await.data.map(|value| value.status),
        Some(CalendarOperationState::Succeeded)
    );
    assert_eq!(backend.operations()?, ["calendar_respond_request", "calendar_send"]);

    let repeated_with_new_id = CalendarRespondInput { idempotency_key: uuid(13), ..input };
    assert_eq!(
        runtime.calendar_respond(repeated_with_new_id).await.data.map(|result| result.status),
        Some(CalendarOperationState::Succeeded)
    );
    assert_eq!(
        backend.operations()?,
        ["calendar_respond_request", "calendar_send", "calendar_respond_request", "calendar_send"]
    );
    Ok(())
}

#[tokio::test]
async fn ordinary_mail_ref_is_rejected_before_journal_or_calendar_mutation() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory, journal) = make_runtime(backend.clone())?;
    let summary = runtime
        .mail_search(MailSearchInput {
            query: "ordinary".into(),
            account_ids: Some(vec!["work".into()]),
            cursor: None,
            limit: Some(1),
        })
        .await
        .data
        .and_then(|page| page.items.into_iter().next())
        .ok_or_else(|| anyhow::anyhow!("ordinary mail is missing"))?;
    let operation_id = uuid(12);

    assert_eq!(
        runtime
            .calendar_respond(CalendarRespondInput {
                scope: None,
                event_ref: summary.mail_ref,
                response: CalendarResponseChoice::Accept,
                comment: String::new(),
                idempotency_key: operation_id.clone(),
            })
            .await
            .error
            .map(|error| error.code),
        Some(ErrorCode::ValidationFailed)
    );
    assert!(journal.lookup(&operation_id)?.is_none());
    assert!(backend.operations()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn safe_notification_failure_is_partial_and_never_retried() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    backend.set_operation_failure(Some("calendar_send"), ErrorCode::ProtocolError)?;
    let (runtime, _directory, journal) = make_runtime(backend.clone())?;
    let input = create_input(true, uuid(20));
    let first = runtime.calendar_create(input.clone()).await;
    let result = first.data.ok_or_else(|| anyhow::anyhow!("partial result is missing"))?;
    assert_eq!(result.status, CalendarOperationState::Partial);
    assert_eq!(result.completed_steps, ["calendar_item"]);
    let record =
        journal.lookup(&uuid(20))?.ok_or_else(|| anyhow::anyhow!("journal record is missing"))?;
    assert_eq!(record.status, OperationStatus::Partial);
    assert_eq!(record.completed_steps, 1);

    backend.set_operation_failure(None, ErrorCode::ProtocolError)?;
    let repeated = runtime.calendar_create(input).await;
    assert_eq!(repeated.data.map(|value| value.status), Some(CalendarOperationState::Partial));
    assert_eq!(backend.operations()?, ["calendar_create_item"]);
    Ok(())
}

#[tokio::test]
async fn unknown_mutation_outcome_is_durable_and_not_replayed() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    backend.set_operation_failure(Some("calendar_create_item"), ErrorCode::OutcomeUnknown)?;
    let (runtime, _directory, journal) = make_runtime(backend.clone())?;
    let input = create_input(false, uuid(21));
    let first = runtime.calendar_create(input.clone()).await;
    assert_eq!(first.data.map(|value| value.status), Some(CalendarOperationState::Unknown));
    assert_eq!(
        journal.lookup(&uuid(21))?.map(|record| record.status),
        Some(OperationStatus::Unknown)
    );

    backend.set_operation_failure(None, ErrorCode::OutcomeUnknown)?;
    let repeated = runtime.calendar_create(input).await;
    assert_eq!(repeated.data.map(|value| value.status), Some(CalendarOperationState::Unknown));
    assert!(backend.operations()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn recurrence_and_missing_capabilities_fail_before_mutation() -> anyhow::Result<()> {
    let recurring = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory, journal) = make_runtime(recurring.clone())?;
    let input = CalendarUpdateInput {
        scope: None,
        recurrence: None,
        event_ref: event_ref(&runtime, "recurring").await?,
        subject: Some("Forbidden".into()),
        schedule: None,
        body: None,
        location: None,
        reminder_minutes: None,
        clear_reminder: false,
        busy_status: None,
        attendees: None,
        idempotency_key: uuid(30),
    };
    assert_eq!(
        runtime.calendar_update(input).await.error.map(|error| error.code),
        Some(ErrorCode::ValidationFailed)
    );
    assert_eq!(recurring.source_resolutions(), 1);
    assert!(journal.lookup(&uuid(30))?.is_none());

    let personal_disabled =
        Arc::new(FakeBackend::new("work").with_calendar_capabilities(false, false));
    let (runtime, _directory, journal) = make_runtime(personal_disabled.clone())?;
    assert_eq!(
        runtime.calendar_create(create_input(false, uuid(31))).await.error.map(|error| error.code),
        Some(ErrorCode::FeatureUnavailable)
    );
    assert!(personal_disabled.operations()?.is_empty());
    assert!(journal.lookup(&uuid(31))?.is_none());

    let meeting_disabled =
        Arc::new(FakeBackend::new("work").with_calendar_capabilities(true, false));
    let (runtime, _directory, _) = make_runtime(meeting_disabled.clone())?;
    assert_eq!(
        runtime.calendar_create(create_input(true, uuid(32))).await.error.map(|error| error.code),
        Some(ErrorCode::FeatureUnavailable)
    );
    assert!(meeting_disabled.operations()?.is_empty());
    Ok(())
}

fn make_runtime(
    backend: Arc<FakeBackend>,
) -> anyhow::Result<(Runtime, tempfile::TempDir, Arc<MemoryJournal>)> {
    let directory = tempfile::tempdir()?;
    let boundary: Arc<dyn AccountBackend> = backend;
    let journal = Arc::new(MemoryJournal::default());
    let journal_boundary: Arc<dyn OperationJournal> = journal.clone();
    let runtime = Runtime::with_dependencies(
        vec![boundary],
        journal_boundary,
        Arc::new(FixedClock::new(chrono::DateTime::UNIX_EPOCH)),
        Arc::new(SequenceIds::default()),
        vec![7; 32],
        directory.path().join("attachments"),
    )?;
    Ok((runtime, directory, journal))
}

async fn event_ref(runtime: &Runtime, query: &str) -> anyhow::Result<String> {
    runtime
        .calendar_search(CalendarSearchInput {
            query: Some(query.into()),
            date_from: None,
            date_to: None,
            time_zone: None,
            account_ids: Some(vec!["work".into()]),
            limit: Some(1),
        })
        .await
        .data
        .and_then(|result| result.items.into_iter().next())
        .map(|event| event.event_ref)
        .ok_or_else(|| anyhow::anyhow!("calendar search returned no event"))
}

fn create_input(meeting: bool, idempotency_key: String) -> CalendarCreateInput {
    CalendarCreateInput {
        recurrence: None,
        account_id: "work".into(),
        subject: "Stable release test".into(),
        schedule: CalendarScheduleInput::AllDay {
            start_date: "2026-08-24".into(),
            end_date: "2026-08-25".into(),
            time_zone: "UTC".into(),
        },
        body: "Fixture body".into(),
        location: "Room 1".into(),
        reminder_minutes: Some(15),
        busy_status: CalendarBusyStatus::Busy,
        attendees: meeting
            .then(|| CalendarAttendeeInput {
                email: "guest@example.invalid".into(),
                name: Some("Guest".into()),
                role: CalendarAttendeeRole::Required,
            })
            .into_iter()
            .collect(),
        idempotency_key,
    }
}

fn uuid(index: u8) -> String {
    format!("11111111-2222-4333-8444-5555555555{index:02}")
}
