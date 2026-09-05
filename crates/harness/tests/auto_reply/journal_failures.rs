use super::*;
use eas_mail_mcp::{ApiResponse, AutoReplyOperationResult, OperationJournal};

#[path = "fault_journal.rs"]
mod fault_journal;
use fault_journal::FaultJournal;

#[tokio::test]
async fn failed_journal_after_acknowledged_or_ambiguous_set_retains_uuid_and_never_resends()
-> anyhow::Result<()> {
    for set_error in [None, Some(ErrorCode::OutcomeUnknown), Some(ErrorCode::AccessDenied)] {
        let fixture = fixture()?;
        fixture.backend.configure_auto_reply(false, None, set_error)?;
        fixture.journal.set_finish_failure(true);
        let input = enabled(20);
        let response = fixture.runtime.mail_set_auto_reply(input.clone()).await;
        assert_unknown(&response, &input)?;
        assert_eq!(fixture.backend.auto_reply_attempts()?, 1);
        fixture.journal.set_finish_failure(false);
        let replay = fixture.runtime.mail_set_auto_reply(input.clone()).await;
        assert_eq!(replay.data.map(|value| value.status), Some(AutoReplyOperationState::Unknown));
        assert_eq!(fixture.backend.auto_reply_attempts()?, 1);
        assert_eq!(
            fixture.journal.lookup(&input.idempotency_key)?.map(|r| r.status),
            Some(OperationStatus::Pending)
        );
    }
    Ok(())
}

#[tokio::test]
async fn verified_set_with_final_finish_failure_preserves_acknowledgement_on_replay()
-> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let journal = Arc::new(FaultJournal::new(Some(OperationStatus::Succeeded), false));
    let (runtime, _directory) = with_journal(backend.clone(), journal.clone())?;
    let input = enabled(21);
    assert_unknown(&runtime.mail_set_auto_reply(input.clone()).await, &input)?;
    let entry = journal.lookup(&input.idempotency_key)?.ok_or_else(|| anyhow::anyhow!("record"))?;
    assert_eq!((entry.status, entry.completed_steps), (OperationStatus::Partial, 1));
    let replay = runtime.mail_set_auto_reply(input.clone()).await;
    assert_partial_warning(&replay, &input)?;
    assert!(replay.data.is_some_and(
        |value| value.status == AutoReplyOperationState::Partial && value.settings.is_none()
    ));
    assert_eq!(backend.auto_reply_attempts()?, 1);
    Ok(())
}

#[tokio::test]
async fn verification_errors_keep_partial_warning_and_acknowledgement_without_resending()
-> anyhow::Result<()> {
    for error in [ErrorCode::NetworkUnreachable, ErrorCode::ProtocolError, ErrorCode::StorageError]
    {
        let fixture = fixture()?;
        fixture.backend.configure_auto_reply(false, Some(error), None)?;
        let input = enabled(22);
        for _ in 0..2 {
            let response = fixture.runtime.mail_set_auto_reply(input.clone()).await;
            assert_partial_warning(&response, &input)?;
            assert!(
                response.data.is_some_and(|value| value.status == AutoReplyOperationState::Partial
                    && value.settings.is_none())
            );
        }
        let entry = fixture
            .journal
            .lookup(&input.idempotency_key)?
            .ok_or_else(|| anyhow::anyhow!("record"))?;
        assert_eq!((entry.status, entry.completed_steps), (OperationStatus::Partial, 1));
        assert_eq!(fixture.backend.auto_reply_attempts()?, 1);
    }
    Ok(())
}

#[tokio::test]
async fn remote_wipe_purge_failure_retains_uuid_and_blocks_further_updates() -> anyhow::Result<()> {
    for during_set in [true, false] {
        let backend = Arc::new(FakeBackend::new("work"));
        backend.configure_auto_reply(
            false,
            (!during_set).then_some(ErrorCode::RemoteWipe),
            during_set.then_some(ErrorCode::RemoteWipe),
        )?;
        let journal = Arc::new(FaultJournal::new(None, true));
        let (runtime, _directory) = with_journal(backend.clone(), journal.clone())?;
        let input = enabled(23);
        assert_unknown(&runtime.mail_set_auto_reply(input.clone()).await, &input)?;
        let entry =
            journal.lookup(&input.idempotency_key)?.ok_or_else(|| anyhow::anyhow!("record"))?;
        let expected = if during_set { OperationStatus::Pending } else { OperationStatus::Partial };
        assert_eq!(entry.status, expected);
        let replay = runtime.mail_set_auto_reply(input).await;
        assert!(replay.data.is_some());
        let new_write = runtime.mail_set_auto_reply(enabled(24)).await;
        assert_eq!(new_write.error.map(|error| error.code), Some(ErrorCode::RemoteWipe));
        assert_eq!(backend.auto_reply_attempts()?, 1);
    }
    Ok(())
}

fn assert_unknown(
    response: &ApiResponse<AutoReplyOperationResult>,
    input: &AutoReplySetInput,
) -> anyhow::Result<()> {
    let error = response.error.as_ref().ok_or_else(|| anyhow::anyhow!("missing unknown error"))?;
    assert_eq!(error.code, ErrorCode::OutcomeUnknown);
    assert_eq!(error.operation_id.as_deref(), Some(input.idempotency_key.as_str()));
    assert_eq!(error.account_id.as_deref(), Some(input.account_id.as_str()));
    assert!(!error.retryable);
    assert!(response.data.is_none());
    Ok(())
}

pub(super) fn assert_partial_warning(
    response: &ApiResponse<AutoReplyOperationResult>,
    input: &AutoReplySetInput,
) -> anyhow::Result<()> {
    assert!(response.error.is_none());
    assert_eq!(response.warnings.len(), 1);
    let warning = response.warnings.first().ok_or_else(|| anyhow::anyhow!("warning"))?;
    assert_eq!(warning.code, "PARTIAL_WRITE");
    assert_eq!(warning.account_id, input.account_id);
    assert_eq!(warning.operation_id.as_deref(), Some(input.idempotency_key.as_str()));
    assert!(!warning.retryable);
    assert!(!warning.message.contains("Internal fixture"));
    Ok(())
}

fn with_journal(
    backend: Arc<FakeBackend>,
    journal: Arc<dyn OperationJournal>,
) -> anyhow::Result<(Runtime, tempfile::TempDir)> {
    let directory = tempfile::tempdir()?;
    let now = Utc
        .with_ymd_and_hms(2026, 9, 4, 12, 0, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("date"))?;
    let runtime = Runtime::with_dependencies(
        vec![backend],
        journal,
        Arc::new(FixedClock::new(now)),
        Arc::new(SequenceIds::default()),
        vec![7; 32],
        directory.path().join("attachments"),
    )?;
    Ok((runtime, directory))
}
