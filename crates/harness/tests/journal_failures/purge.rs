use super::*;
use eas_mail_mcp::{ApiResponse, MailListInput, MailMoveInput};

#[path = "../auto_reply/fault_journal.rs"]
mod fault_journal;
use fault_journal::FaultJournal;

#[tokio::test]
async fn remote_wipe_cleanup_failure_after_mail_and_calendar_mutations_keeps_uuid()
-> anyhow::Result<()> {
    for kind in ["mail_send", "mail_move", "calendar_send"] {
        let backend = Arc::new(FakeBackend::new("work"));
        backend.set_operation_failure(Some(kind), ErrorCode::RemoteWipe)?;
        let journal = Arc::new(FaultJournal::new(None, true));
        let directory = tempfile::tempdir()?;
        let runtime = Runtime::with_dependencies(
            vec![backend.clone()],
            journal.clone(),
            Arc::new(SystemClock),
            Arc::new(RandomIds),
            vec![7; 32],
            directory.path().join("attachments"),
        )?;
        let operation_id = match kind {
            "mail_send" => {
                let input = send_input()?;
                check(runtime.mail_send(input.clone()).await, &input.idempotency_key)?;
                input.idempotency_key
            }
            "mail_move" => {
                let mail_ref = runtime
                    .mail_list(MailListInput::default())
                    .await
                    .data
                    .and_then(|page| page.items.first().map(|item| item.mail_ref.clone()))
                    .ok_or_else(|| anyhow::anyhow!("mail ref"))?;
                let input = MailMoveInput {
                    mail_ref,
                    destination_folder_id: "archive".into(),
                    idempotency_key: "00000000-0000-4000-8000-000000000003".into(),
                };
                check(runtime.mail_move(input.clone()).await, &input.idempotency_key)?;
                input.idempotency_key
            }
            _ => {
                let input = calendar_input()?;
                check(runtime.calendar_create(input.clone()).await, &input.idempotency_key)?;
                assert_eq!(backend.operations()?, ["calendar_create_item"]);
                input.idempotency_key
            }
        };
        let record =
            journal.lookup(&operation_id)?.ok_or_else(|| anyhow::anyhow!("retained record"))?;
        assert_eq!(record.status, OperationStatus::Pending);
        assert_eq!(record.completed_steps != 0, kind == "calendar_send");
        backend.set_operation_failure(None, ErrorCode::RemoteWipe)?;
        let mut next = send_input()?;
        next.idempotency_key = "00000000-0000-4000-8000-000000000004".into();
        assert_eq!(
            runtime.mail_send(next).await.error.map(|error| error.code),
            Some(ErrorCode::RemoteWipe)
        );
        assert!(!backend.operations()?.iter().any(|name| name == "mail_send"));
    }
    Ok(())
}

fn check<T>(response: ApiResponse<T>, operation_id: &str) -> anyhow::Result<()> {
    let error = response.error.ok_or_else(|| anyhow::anyhow!("missing purge failure"))?;
    assert_eq!(error.code, ErrorCode::OutcomeUnknown);
    assert_eq!(error.operation_id.as_deref(), Some(operation_id));
    assert_eq!(error.account_id.as_deref(), Some("work"));
    assert!(!error.retryable);
    Ok(())
}
