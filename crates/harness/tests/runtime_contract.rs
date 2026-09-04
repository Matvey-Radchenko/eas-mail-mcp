use std::sync::Arc;

use eas_mail_mcp::backend::AccountBackend;
use eas_mail_mcp::{
    AccountSelection, CalendarAvailabilityInput, CalendarFindSlotsInput, CalendarGetInput,
    CalendarSearchInput, ErrorCode, MailForwardInput, MailListInput, MailReplyInput,
    MailSearchInput, MarkReadInput, OperationJournal, OperationState, Runtime, ScheduleWeekday,
    WorkingHoursInput,
};
use eas_mail_mcp_harness::{FakeBackend, FixedClock, MemoryJournal, SequenceIds};

#[tokio::test]
async fn read_contract_covers_selection_sync_search_and_calendar() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work").with_mail_count(2));
    let (runtime, _directory) = runtime(vec![backend])?;
    assert_eq!(runtime.accounts_list().data.map(|data| data.accounts.len()), Some(1));
    let folders = runtime
        .folders_list(AccountSelection::default())
        .await
        .data
        .ok_or_else(|| anyhow::anyhow!("folders_list returned no data"))?;
    assert_eq!(folders.folders.len(), 4);
    assert!(folders.folders.iter().any(|folder| folder.role == "inbox"));
    assert!(folders.folders.iter().any(|folder| folder.role == "calendar"));
    assert_eq!(
        runtime.sync_status(AccountSelection::default()).data.map(|data| data.reports.len()),
        Some(0)
    );
    let response = runtime.sync_now(AccountSelection::default()).await;
    assert!(response.error.is_none());
    assert_eq!(response.data.map(|data| data.reports.len()), Some(1));
    assert_eq!(
        runtime.sync_status(AccountSelection::default()).data.map(|data| data.reports.len()),
        Some(1)
    );

    let search = runtime
        .mail_search(MailSearchInput {
            filters: Default::default(),
            query: "quarterly".into(),
            account_ids: None,
            cursor: None,
            limit: Some(1),
        })
        .await;
    assert_eq!(search.data.as_ref().map(|data| data.items.len()), Some(1));
    let cursor = search.data.and_then(|data| data.next_cursor);
    assert!(cursor.is_some());
    assert_eq!(
        runtime
            .mail_search(MailSearchInput {
                filters: Default::default(),
                query: "ignored for cursor".into(),
                account_ids: None,
                cursor,
                limit: Some(1),
            })
            .await
            .data
            .map(|data| data.items.len()),
        Some(1)
    );
    assert_eq!(
        runtime
            .mail_search(MailSearchInput {
                filters: Default::default(),
                query: " ".into(),
                account_ids: None,
                cursor: None,
                limit: None,
            })
            .await
            .error
            .map(|error| error.code),
        Some(ErrorCode::ValidationFailed)
    );

    exercise_calendar(&runtime).await?;
    Ok(())
}

async fn exercise_calendar(runtime: &Runtime) -> anyhow::Result<()> {
    let availability = runtime
        .calendar_availability(CalendarAvailabilityInput {
            account_id: None,
            participants: vec!["work@example.invalid".into()],
            date_from: "2026-08-03".into(),
            date_to: "2026-08-03".into(),
            time_zone: "UTC".into(),
            working_hours: working_hours(),
        })
        .await;
    assert_eq!(availability.data.map(|data| data.participants.len()), Some(1));
    let slots = runtime
        .calendar_find_slots(CalendarFindSlotsInput {
            account_id: None,
            participants: vec!["work@example.invalid".into()],
            date_from: "2026-08-03".into(),
            date_to: "2026-08-03".into(),
            time_zone: "UTC".into(),
            working_hours: working_hours(),
            duration_minutes: 60,
            allow_tentative: false,
            participant_options: Vec::new(),
            buffer_minutes: 0,
            limit: None,
        })
        .await;
    assert_eq!(slots.data.map(|data| data.windows.len()), Some(1));
    let events = runtime
        .calendar_search(CalendarSearchInput {
            query: Some("planning".into()),
            date_from: None,
            date_to: None,
            time_zone: None,
            account_ids: None,
            limit: None,
        })
        .await;
    let event_ref = events
        .data
        .and_then(|data| data.items.into_iter().next())
        .map(|event| event.event_ref)
        .ok_or_else(|| anyhow::anyhow!("calendar event is missing"))?;
    let event = runtime
        .calendar_get(CalendarGetInput { event_ref, body_limit: None })
        .await
        .data
        .ok_or_else(|| anyhow::anyhow!("calendar_get returned no data"))?;
    assert_eq!(event.uid, "event-uid@example.invalid");
    assert!(event.can_update && event.can_cancel);
    assert!(!event.can_delete && !event.can_respond);

    exercise_agenda(runtime).await?;

    let received_ref = runtime
        .calendar_search(CalendarSearchInput {
            query: Some("received".into()),
            date_from: None,
            date_to: None,
            time_zone: None,
            account_ids: None,
            limit: Some(1),
        })
        .await
        .data
        .and_then(|data| data.items.into_iter().next())
        .map(|event| event.event_ref)
        .ok_or_else(|| anyhow::anyhow!("received meeting is missing"))?;
    let received = runtime
        .calendar_get(CalendarGetInput { event_ref: received_ref, body_limit: None })
        .await
        .data
        .ok_or_else(|| anyhow::anyhow!("received calendar_get returned no data"))?;
    assert!(received.can_respond);
    assert!(!received.can_update && !received.can_delete && !received.can_cancel);
    assert_eq!(
        runtime
            .calendar_search(CalendarSearchInput {
                query: Some(" ".into()),
                date_from: None,
                date_to: None,
                time_zone: None,
                account_ids: None,
                limit: None,
            })
            .await
            .error
            .map(|error| error.code),
        Some(ErrorCode::ValidationFailed)
    );
    Ok(())
}

async fn exercise_agenda(runtime: &Runtime) -> anyhow::Result<()> {
    let agenda = runtime
        .calendar_search(CalendarSearchInput {
            query: None,
            date_from: Some("2023-11-15".into()),
            date_to: Some("2023-11-15".into()),
            time_zone: Some("UTC".into()),
            account_ids: None,
            limit: Some(10),
        })
        .await
        .data
        .ok_or_else(|| anyhow::anyhow!("calendar agenda returned no data"))?;
    assert_eq!(agenda.items.len(), 1);
    let first = agenda.items.first().ok_or_else(|| anyhow::anyhow!("agenda item is missing"))?;
    assert!(!first.recurring);
    Ok(())
}

fn working_hours() -> Vec<WorkingHoursInput> {
    vec![WorkingHoursInput {
        weekdays: vec![ScheduleWeekday::Mon],
        start: "09:00".into(),
        end: "18:00".into(),
    }]
}

#[tokio::test]
async fn all_write_tools_return_durable_success() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(vec![backend.clone()])?;
    let mail_ref = first_mail_ref(&runtime).await?;

    let marked = runtime
        .mail_mark_read(MarkReadInput {
            mail_ref: mail_ref.clone(),
            is_read: true,
            idempotency_key: uuid(1),
        })
        .await;
    assert_eq!(marked.data.map(|value| value.status), Some(OperationState::Succeeded));
    let replied = runtime
        .mail_reply(MailReplyInput {
            attachments: Vec::new(),
            mail_ref: mail_ref.clone(),
            body: "Reply".into(),
            reply_all: true,
            idempotency_key: uuid(2),
        })
        .await;
    assert_eq!(replied.data.map(|value| value.status), Some(OperationState::Succeeded));
    let forwarded = runtime
        .mail_forward(MailForwardInput {
            attachments: Vec::new(),
            mail_ref,
            to: vec!["recipient@example.com".into()],
            cc: Vec::new(),
            bcc: Vec::new(),
            body: "Forward".into(),
            idempotency_key: uuid(3),
        })
        .await;
    assert_eq!(forwarded.data.map(|value| value.status), Some(OperationState::Succeeded));
    assert_eq!(backend.operations()?.len(), 3);
    Ok(())
}

#[tokio::test]
async fn write_failures_are_journaled_and_replayed_without_retry() -> anyhow::Result<()> {
    for (code, expected) in [
        (ErrorCode::OutcomeUnknown, OperationState::Unknown),
        (ErrorCode::ProtocolError, OperationState::Failed),
    ] {
        let backend = Arc::new(FakeBackend::new("work"));
        let (runtime, _directory) = runtime(vec![backend.clone()])?;
        let mail_ref = first_mail_ref(&runtime).await?;
        backend.set_operation_failure(Some("mail_mark_read"), code)?;
        let input = MarkReadInput { mail_ref, is_read: true, idempotency_key: uuid(4) };
        let first = runtime.mail_mark_read(input.clone()).await;
        assert_eq!(first.error.map(|error| error.code), Some(code));
        backend.set_operation_failure(None, code)?;
        let repeated = runtime.mail_mark_read(input).await;
        assert_eq!(repeated.data.map(|value| value.status), Some(expected));
        assert!(backend.operations()?.is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn runtime_rejects_invalid_boundaries_and_account_selection() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let empty = Runtime::with_dependencies(
        Vec::new(),
        Arc::new(MemoryJournal::default()),
        Arc::new(FixedClock::new(chrono::DateTime::UNIX_EPOCH)),
        Arc::new(SequenceIds::default()),
        vec![7; 32],
        directory.path().join("empty"),
    )?;
    assert_eq!(
        empty.folders_list(AccountSelection::default()).await.error.map(|error| error.code),
        Some(ErrorCode::ConfigInvalid)
    );

    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(vec![backend.clone()])?;
    assert_eq!(
        runtime
            .folders_list(AccountSelection { account_ids: Some(vec!["missing".into()]) })
            .await
            .error
            .map(|error| error.code),
        Some(ErrorCode::ValidationFailed)
    );
    let duplicate = Runtime::with_dependencies(
        vec![backend.clone(), backend],
        Arc::new(MemoryJournal::default()),
        Arc::new(FixedClock::new(chrono::DateTime::UNIX_EPOCH)),
        Arc::new(SequenceIds::default()),
        vec![7; 32],
        directory.path().join("duplicate"),
    );
    assert!(duplicate.is_err_and(|error| error.envelope.code == ErrorCode::ConfigInvalid));
    let bad_key = Runtime::with_dependencies(
        Vec::new(),
        Arc::new(MemoryJournal::default()),
        Arc::new(FixedClock::new(chrono::DateTime::UNIX_EPOCH)),
        Arc::new(SequenceIds::default()),
        vec![7; 31],
        directory.path().join("bad-key"),
    );
    assert!(bad_key.is_err_and(|error| error.envelope.code == ErrorCode::StorageError));
    Ok(())
}

#[tokio::test]
async fn calendar_account_selection_requires_one_domain_match() -> anyhow::Result<()> {
    let alpha = Arc::new(
        FakeBackend::new("alpha").with_identity("owner@alpha.invalid", &["alpha.invalid"]),
    );
    let beta =
        Arc::new(FakeBackend::new("beta").with_identity("owner@beta.invalid", &["beta.invalid"]));
    let (runtime, _directory) = runtime(vec![alpha, beta])?;
    let selected = runtime
        .calendar_availability(CalendarAvailabilityInput {
            account_id: None,
            participants: vec!["person@alpha.invalid".into()],
            date_from: "2026-08-03".into(),
            date_to: "2026-08-03".into(),
            time_zone: "UTC".into(),
            working_hours: working_hours(),
        })
        .await;
    assert_eq!(
        selected.data.map(|value| value.account_id.as_str().to_owned()),
        Some("alpha".into())
    );

    let ambiguous = runtime
        .calendar_availability(CalendarAvailabilityInput {
            account_id: None,
            participants: vec!["Directory Name".into()],
            date_from: "2026-08-03".into(),
            date_to: "2026-08-03".into(),
            time_zone: "UTC".into(),
            working_hours: working_hours(),
        })
        .await;
    assert_eq!(ambiguous.error.map(|value| value.code), Some(ErrorCode::AccountSelectionRequired));
    Ok(())
}

#[tokio::test]
async fn account_write_gate_blocks_mutations() -> anyhow::Result<()> {
    let disabled = Arc::new(FakeBackend::new("work").with_writes_enabled(false));
    let (runtime, _directory) = runtime(vec![disabled])?;
    let mail_ref = first_mail_ref(&runtime).await?;
    assert_eq!(
        runtime
            .mail_mark_read(MarkReadInput { mail_ref, is_read: true, idempotency_key: uuid(5) })
            .await
            .error
            .map(|error| error.code),
        Some(ErrorCode::ValidationFailed)
    );
    Ok(())
}

fn runtime(backends: Vec<Arc<FakeBackend>>) -> anyhow::Result<(Runtime, tempfile::TempDir)> {
    let directory = tempfile::tempdir()?;
    let boundaries =
        backends.into_iter().map(|backend| -> Arc<dyn AccountBackend> { backend }).collect();
    let journal: Arc<dyn OperationJournal> = Arc::new(MemoryJournal::default());
    Ok((
        Runtime::with_dependencies(
            boundaries,
            journal,
            Arc::new(FixedClock::new(chrono::DateTime::UNIX_EPOCH)),
            Arc::new(SequenceIds::default()),
            vec![7; 32],
            directory.path().join("attachments"),
        )?,
        directory,
    ))
}

async fn first_mail_ref(runtime: &Runtime) -> anyhow::Result<String> {
    runtime
        .mail_list(MailListInput::default())
        .await
        .data
        .and_then(|data| data.items.into_iter().next())
        .map(|mail| mail.mail_ref)
        .ok_or_else(|| anyhow::anyhow!("mail list is empty"))
}

fn uuid(index: u8) -> String {
    format!("11111111-2222-4333-8444-5555555555{index:02}")
}
