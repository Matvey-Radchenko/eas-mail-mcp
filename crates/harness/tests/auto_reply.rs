use std::sync::Arc;

use chrono::{TimeZone as _, Utc};
use eas_mail_mcp::{
    AutoReplyExternalAudience, AutoReplyGetInput, AutoReplyOperationState, AutoReplySetInput,
    AutoReplyState, ErrorCode, OperationJournal as _, OperationStatus, Runtime,
};
use eas_mail_mcp_harness::{FakeBackend, FixedClock, MemoryJournal, SequenceIds};

#[path = "auto_reply/journal_failures.rs"]
mod journal_failures;

#[tokio::test]
async fn automatic_replies_verify_and_replay_without_sending_again() -> anyhow::Result<()> {
    let fixture = fixture()?;
    let input = enabled(1);
    let first = fixture.runtime.mail_set_auto_reply(input.clone()).await;
    assert!(first.error.is_none());
    let first = first.data.ok_or_else(|| anyhow::anyhow!("missing update result"))?;
    assert_eq!(first.status, AutoReplyOperationState::Succeeded);
    let settings = first.settings.ok_or_else(|| anyhow::anyhow!("missing read-back"))?;
    assert_eq!(settings.state, AutoReplyState::Enabled);
    assert!(settings.internal.is_some_and(|item| item.enabled && item.message.as_deref() == Some("Internal fixture")));
    assert!(settings.external_known.is_some_and(|item| !item.enabled));
    assert!(settings.external_unknown.is_some_and(|item| !item.enabled));
    let replay = fixture.runtime.mail_set_auto_reply(input.clone()).await;
    assert!(replay.data.is_some_and(
        |value| value.status == AutoReplyOperationState::Succeeded && value.settings.is_none()
    ));
    assert_eq!(fixture.backend.auto_reply_attempts()?, 1);
    let mut changed = input;
    changed.internal_message = Some("changed".into());
    assert!(
        fixture
            .runtime
            .mail_set_auto_reply(changed)
            .await
            .error
            .is_some_and(|error| error.code == ErrorCode::IdempotencyConflict)
    );
    Ok(())
}

#[tokio::test]
async fn administrator_ignored_external_settings_are_partial_and_not_retried() -> anyhow::Result<()>
{
    let fixture = fixture()?;
    fixture.backend.configure_auto_reply(true, None, None)?;
    let mut input = enabled(2);
    input.external_audience = AutoReplyExternalAudience::All;
    input.external_message = Some("External fixture".into());
    let response = fixture.runtime.mail_set_auto_reply(input.clone()).await;
    journal_failures::assert_partial_warning(&response, &input)?;
    let result = response.data.ok_or_else(|| anyhow::anyhow!("missing partial result"))?;
    assert_eq!(result.status, AutoReplyOperationState::Partial);
    assert!(
        result
            .settings
            .is_some_and(|value| value.external_unknown.is_some_and(|item| !item.enabled))
    );
    let record = fixture
        .journal
        .lookup(&input.idempotency_key)?
        .ok_or_else(|| anyhow::anyhow!("missing record"))?;
    assert_eq!(record.status, OperationStatus::Partial);
    assert_eq!(record.completed_steps, 1);
    assert!(!record.payload_hmac.contains("External fixture"));
    let replay = fixture.runtime.mail_set_auto_reply(input.clone()).await;
    journal_failures::assert_partial_warning(&replay, &input)?;
    assert!(replay.data.is_some_and(|value| value.status == AutoReplyOperationState::Partial));
    assert_eq!(fixture.backend.auto_reply_attempts()?, 1);
    Ok(())
}

#[tokio::test]
async fn failed_readback_preserves_acknowledgement_and_unknown_set_never_retries()
-> anyhow::Result<()> {
    let fixture = fixture()?;
    fixture.backend.configure_auto_reply(false, Some(ErrorCode::NetworkUnreachable), None)?;
    let response = fixture.runtime.mail_set_auto_reply(enabled(3)).await;
    journal_failures::assert_partial_warning(&response, &enabled(3))?;
    assert!(response.data.is_some_and(
        |value| value.status == AutoReplyOperationState::Partial && value.settings.is_none()
    ));
    fixture.backend.configure_auto_reply(false, None, Some(ErrorCode::OutcomeUnknown))?;
    let input = enabled(4);
    let response = fixture.runtime.mail_set_auto_reply(input.clone()).await;
    assert!(response.error.is_some_and(|error| error.code == ErrorCode::OutcomeUnknown
        && error.operation_id.as_deref() == Some(input.idempotency_key.as_str())));
    assert!(
        fixture
            .runtime
            .mail_set_auto_reply(input)
            .await
            .data
            .is_some_and(|value| value.status == AutoReplyOperationState::Unknown)
    );
    assert_eq!(fixture.backend.auto_reply_attempts()?, 2);
    Ok(())
}

#[tokio::test]
async fn schedule_requires_offsets_and_ordered_dates_and_disable_preserves_messages()
-> anyhow::Result<()> {
    let fixture = fixture()?;
    let mut input = enabled(5);
    input.state = AutoReplyState::Scheduled;
    assert!(
        fixture
            .runtime
            .mail_set_auto_reply(input.clone())
            .await
            .error
            .is_some_and(|error| error.code == ErrorCode::ValidationFailed)
    );
    input.starts_at = Some(
        Utc.with_ymd_and_hms(2026, 9, 7, 7, 0, 0)
            .single()
            .ok_or_else(|| anyhow::anyhow!("date"))?,
    );
    input.ends_at = input.starts_at;
    assert!(fixture.runtime.mail_set_auto_reply(input.clone()).await.error.is_some());
    input.ends_at = input.starts_at.map(|date| date + chrono::Duration::days(5));
    assert!(
        fixture
            .runtime
            .mail_set_auto_reply(input)
            .await
            .data
            .is_some_and(|value| value.status == AutoReplyOperationState::Succeeded)
    );
    let mut disable = enabled(6);
    disable.state = AutoReplyState::Disabled;
    disable.internal_message = None;
    assert!(
        fixture
            .runtime
            .mail_set_auto_reply(disable)
            .await
            .data
            .is_some_and(|value| value.status == AutoReplyOperationState::Succeeded)
    );
    let settings =
        fixture.runtime.mail_get_auto_reply(AutoReplyGetInput { account_id: "work".into() }).await;
    assert!(settings.data.is_some_and(|value| {
        value.state == AutoReplyState::Disabled
            && value
                .internal
                .is_some_and(|message| message.message.as_deref() == Some("Internal fixture"))
    }));
    let invalid = serde_json::json!({"account_id":"work", "state":"scheduled", "starts_at":"2026-09-07T09:00:00", "idempotency_key":uuid(7)});
    assert!(serde_json::from_value::<AutoReplySetInput>(invalid).is_err());
    Ok(())
}

#[tokio::test]
async fn invalid_messages_and_disabled_writes_never_send_settings() -> anyhow::Result<()> {
    let fixture = fixture()?;
    let mut empty = enabled(8);
    empty.internal_message = None;
    let mut external_missing = enabled(9);
    external_missing.external_audience = AutoReplyExternalAudience::Known;
    let mut control = enabled(10);
    control.internal_message = Some("escape\u{1b}[31m".into());
    for input in [empty, external_missing, control] {
        assert!(
            fixture
                .runtime
                .mail_set_auto_reply(input)
                .await
                .error
                .is_some_and(|error| error.code == ErrorCode::ValidationFailed)
        );
    }
    assert_eq!(fixture.backend.auto_reply_attempts()?, 0);
    let backend = Arc::new(FakeBackend::new("work").with_writes_enabled(false));
    let readonly = fixture_with_backend(backend.clone())?;
    assert!(readonly.runtime.mail_set_auto_reply(enabled(11)).await.error.is_some());
    assert_eq!(backend.auto_reply_attempts()?, 0);
    Ok(())
}

struct Fixture {
    runtime: Runtime,
    backend: Arc<FakeBackend>,
    journal: Arc<MemoryJournal>,
    _directory: tempfile::TempDir,
}

fn fixture() -> anyhow::Result<Fixture> {
    fixture_with_backend(Arc::new(FakeBackend::new("work")))
}

fn fixture_with_backend(backend: Arc<FakeBackend>) -> anyhow::Result<Fixture> {
    let directory = tempfile::tempdir()?;
    let journal = Arc::new(MemoryJournal::default());
    let now = Utc
        .with_ymd_and_hms(2026, 9, 4, 12, 0, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("date"))?;
    let runtime = Runtime::with_dependencies(
        vec![backend.clone()],
        journal.clone(),
        Arc::new(FixedClock::new(now)),
        Arc::new(SequenceIds::default()),
        vec![7; 32],
        directory.path().join("attachments"),
    )?;
    Ok(Fixture { runtime, backend, journal, _directory: directory })
}

fn enabled(id: u8) -> AutoReplySetInput {
    AutoReplySetInput {
        account_id: "work".into(),
        state: AutoReplyState::Enabled,
        starts_at: None,
        ends_at: None,
        internal_message: Some("Internal fixture".into()),
        external_audience: AutoReplyExternalAudience::None,
        external_message: None,
        idempotency_key: uuid(id),
    }
}

fn uuid(value: u8) -> String {
    format!("00000000-0000-4000-8000-{value:012}")
}
