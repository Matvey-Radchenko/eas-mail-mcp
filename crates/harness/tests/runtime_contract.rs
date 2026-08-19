use std::sync::Arc;

use eas_mail_mcp::backend::AccountBackend;
use eas_mail_mcp::{
    AccountSelection, CalendarGetInput, CalendarListInput, CalendarSearchInput, ErrorCode,
    MailForwardInput, MailListInput, MailReplyInput, MailSearchInput, MarkReadInput,
    OperationJournal, OperationState, Runtime, SyncInput, SyncScope,
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
    assert_eq!(folders.folders.len(), 2);
    assert!(folders.folders.iter().any(|folder| folder.role == "inbox"));
    assert!(folders.folders.iter().any(|folder| folder.role == "calendar"));
    assert_eq!(
        runtime.sync_status(AccountSelection::default()).data.map(|data| data.reports.len()),
        Some(0)
    );
    for scope in [SyncScope::Mail, SyncScope::Calendar, SyncScope::All] {
        let response = runtime.sync_now(SyncInput { account_ids: None, scope }).await;
        assert!(response.error.is_none());
        assert_eq!(response.data.map(|data| data.reports.len()), Some(1));
    }
    assert_eq!(
        runtime.sync_status(AccountSelection::default()).data.map(|data| data.reports.len()),
        Some(1)
    );

    let search = runtime
        .mail_search(MailSearchInput {
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

    let events = runtime.calendar_list(CalendarListInput::default()).await;
    let event_ref = events
        .data
        .and_then(|data| data.items.into_iter().next())
        .map(|event| event.event_ref)
        .ok_or_else(|| anyhow::anyhow!("calendar event is missing"))?;
    assert!(runtime.calendar_get(CalendarGetInput { event_ref }).error.is_none());
    assert_eq!(
        runtime
            .calendar_search(CalendarSearchInput {
                query: "planning".into(),
                account_ids: None,
                cursor: None,
                limit: None,
            })
            .await
            .data
            .map(|data| data.items.len()),
        Some(1)
    );
    assert_eq!(
        runtime
            .calendar_search(CalendarSearchInput {
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
    Ok(())
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
            mail_ref: mail_ref.clone(),
            body: "Reply".into(),
            reply_all: true,
            idempotency_key: uuid(2),
        })
        .await;
    assert_eq!(replied.data.map(|value| value.status), Some(OperationState::Succeeded));
    let forwarded = runtime
        .mail_forward(MailForwardInput {
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
        backend.set_failure(Some(code))?;
        let input = MarkReadInput { mail_ref, is_read: true, idempotency_key: uuid(4) };
        let first = runtime.mail_mark_read(input.clone()).await;
        assert_eq!(first.error.map(|error| error.code), Some(code));
        backend.set_failure(None)?;
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
