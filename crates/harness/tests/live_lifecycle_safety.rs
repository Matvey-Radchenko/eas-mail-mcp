use std::sync::Arc;

use eas_mail_mcp::backend::AccountBackend;
use eas_mail_mcp::{
    ApiResponse, AppError, ErrorCode, JournalFilter, OperationJournal, OperationResult,
    OperationState, OperationStatus, Runtime, Warning,
};
use eas_mail_mcp_harness::{FakeBackend, FixedClock, MemoryJournal, SequenceIds};

// Exercise the exact legacy driver without compiling or executing its live main.
#[path = "../src/bin/live_harness/artifact_outcome.rs"]
mod artifact_outcome;
#[path = "../src/bin/live_harness/calendar_lifecycle.rs"]
mod calendar_lifecycle;
#[expect(dead_code, reason = "read-only provider checks are outside this deterministic test")]
#[path = "../src/bin/live_harness/checks.rs"]
mod checks;
#[expect(dead_code, reason = "live report and interactive confirmation are not executed in tests")]
#[path = "../src/bin/live_harness/support.rs"]
mod support;
#[path = "../src/bin/live_harness/write_outcome.rs"]
mod write_outcome;

const OPERATION: &str = "11111111-2222-4333-8444-555555555555";

#[test]
fn artifact_mutations_require_acknowledged_success_before_followup() -> anyhow::Result<()> {
    for status in ["succeeded", "failed", "partial", "unknown", "unexpected"] {
        let response = serde_json::json!({"error":null,"warnings":[],"data":{
            "status":status,"operation_id":OPERATION,"message":"private-message"}});
        let result = artifact_outcome::validate("calendar_create", &response);
        if status == "succeeded" {
            result?;
        } else {
            let error = failed(result)?.context("artifact lifecycle");
            assert_eq!(write_outcome::must_stop(&error), status != "failed");
            assert!(format!("{error:#}").contains(OPERATION));
            assert!(!format!("{error:#}").contains("private-message"));
        }
    }
    for code in ["PARTIAL_WRITE", "OUTCOME_UNKNOWN"] {
        let response = serde_json::json!({"error":null,"data":{},"warnings":[{
            "code":code,"operation_id":OPERATION,"message":"private-message"}]});
        let error = failed(artifact_outcome::validate("calendar_get", &response))?;
        assert!(write_outcome::must_stop(&error));
    }
    let response = serde_json::json!({"error":{"code":"OUTCOME_UNKNOWN",
        "operation_id":OPERATION,"message":"private-message"},"data":null});
    assert!(write_outcome::must_stop(&failed(artifact_outcome::validate(
        "calendar_delete",
        &response
    ))?));
    Ok(())
}

#[tokio::test]
async fn unknown_personal_update_stops_without_delete_or_next_fixture() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    backend.set_operation_failure(Some("calendar_update_item"), ErrorCode::OutcomeUnknown)?;
    let (runtime, _directory, journal) = runtime(backend.clone())?;
    let error = failed(calendar_lifecycle::check_personal_events(&runtime, "work").await)?;
    assert!(write_outcome::must_stop(&error));
    assert_eq!(backend.operations()?, ["calendar_create_item"]);
    assert_retained(&journal, OperationStatus::Unknown, &error)?;
    Ok(())
}

#[tokio::test]
async fn partial_or_unknown_meeting_create_never_cancels_or_starts_reverse_direction()
-> anyhow::Result<()> {
    for (code, status) in [
        (ErrorCode::ProtocolError, OperationStatus::Partial),
        (ErrorCode::OutcomeUnknown, OperationStatus::Unknown),
    ] {
        let backend = Arc::new(FakeBackend::new("work"));
        backend.set_operation_failure(Some("calendar_send"), code)?;
        let (runtime, _directory, journal) = runtime(backend.clone())?;
        let accounts = [account("work"), account("guest")];
        let error =
            failed(calendar_lifecycle::check_meeting_directions(&runtime, &accounts).await)?;
        assert!(write_outcome::must_stop(&error));
        assert_eq!(backend.operations()?, ["calendar_create_item"]);
        assert_retained(&journal, status, &error)?;
    }
    Ok(())
}

#[tokio::test]
async fn definite_personal_failure_still_cleans_confirmed_fixture() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    backend.set_operation_failure(Some("calendar_update_item"), ErrorCode::ProtocolError)?;
    let (runtime, _directory, journal) = runtime(backend.clone())?;
    let error = failed(calendar_lifecycle::check_personal_events(&runtime, "work").await)?;
    assert!(!write_outcome::must_stop(&error));
    assert_eq!(backend.operations()?, ["calendar_create_item", "calendar_delete_item"]);
    assert_retained(&journal, OperationStatus::Failed, &error)?;
    Ok(())
}

#[test]
fn mail_results_require_confirmed_success_and_keep_unknown_uuid() -> anyhow::Result<()> {
    for status in [OperationState::Succeeded, OperationState::Failed, OperationState::Unknown] {
        let response = ApiResponse::success(
            OperationResult {
                operation_id: OPERATION.into(),
                status,
                message: "must not copy this message into diagnostics".into(),
            },
            Vec::new(),
        );
        let result = checks::mail_succeeded(response, "mail_send");
        if status == OperationState::Succeeded {
            assert_eq!(result?.status, status);
        } else {
            let error = failed(result)?;
            assert_eq!(write_outcome::must_stop(&error), status == OperationState::Unknown);
            assert!(error.to_string().contains(OPERATION));
            assert!(!error.to_string().contains("must not copy"));
        }
    }
    Ok(())
}

#[test]
fn incomplete_envelopes_and_warnings_survive_context_without_payload_leaks() -> anyhow::Result<()> {
    let error = failed(checks::required::<()>(
        ApiResponse::failure(
            AppError::new(ErrorCode::OutcomeUnknown, "private-message")
                .operation(OPERATION)
                .envelope,
        ),
        "calendar_update",
    ))?
    .context("outer lifecycle context");
    assert!(write_outcome::must_stop(&error));
    assert!(format!("{error:#}").contains(OPERATION));
    assert!(!format!("{error:#}").contains("private-message"));
    for code in ["PARTIAL_WRITE", "OUTCOME_UNKNOWN"] {
        let response = ApiResponse::success(
            (),
            vec![Warning {
                account_id: "private-account".into(),
                code: code.into(),
                message: "private-message".into(),
                retryable: false,
                remediation: None,
                operation_id: Some(OPERATION.into()),
                retry_after_seconds: None,
            }],
        );
        let error = failed(checks::required(response, "lifecycle"))?.context("outer context");
        assert!(write_outcome::must_stop(&error));
        let diagnostic = format!("{error:#}");
        assert!(diagnostic.contains(OPERATION));
        assert!(!diagnostic.contains("private-"));
    }
    Ok(())
}

fn failed<T>(result: anyhow::Result<T>) -> anyhow::Result<anyhow::Error> {
    result.err().ok_or_else(|| anyhow::anyhow!("expected a stopped lifecycle"))
}

fn assert_retained(
    journal: &MemoryJournal,
    status: OperationStatus,
    error: &anyhow::Error,
) -> anyhow::Result<()> {
    let entries =
        journal.list(&JournalFilter { account_id: None, status: Some(status), limit: 10 })?;
    assert_eq!(entries.len(), 1);
    let entry = entries.first().ok_or_else(|| anyhow::anyhow!("expected retained operation"))?;
    assert!(format!("{error:#}").contains(&entry.record.operation_id));
    Ok(())
}

fn runtime(
    backend: Arc<FakeBackend>,
) -> anyhow::Result<(Runtime, tempfile::TempDir, Arc<MemoryJournal>)> {
    let directory = tempfile::tempdir()?;
    let boundary: Arc<dyn AccountBackend> = backend;
    let journal = Arc::new(MemoryJournal::default());
    let runtime = Runtime::with_dependencies(
        vec![boundary],
        journal.clone(),
        Arc::new(FixedClock::new(chrono::DateTime::UNIX_EPOCH)),
        Arc::new(SequenceIds::default()),
        vec![7; 32],
        directory.path().join("attachments"),
    )?;
    Ok((runtime, directory, journal))
}

fn account(id: &str) -> calendar_lifecycle::LiveAccount {
    calendar_lifecycle::LiveAccount {
        account_id: id.into(),
        profile: "example".into(),
        email: format!("{id}@example.invalid"),
        write_enabled: true,
    }
}
