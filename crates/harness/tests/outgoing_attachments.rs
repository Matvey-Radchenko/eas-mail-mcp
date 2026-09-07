use std::fs;
use std::path::Path;
use std::sync::Arc;

use eas_mail_mcp::{
    ErrorCode, MailForwardInput, MailListInput, MailReplyInput, MailSendInput, OperationJournal,
    OperationState, OutgoingAttachmentInput, RandomIds, Runtime, SqliteJournal, SystemClock,
};
use eas_mail_mcp_harness::FakeBackend;

fn runtime(backend: Arc<FakeBackend>, root: &Path) -> anyhow::Result<Runtime> {
    Ok(Runtime::with_dependencies(
        vec![backend],
        Arc::new(SqliteJournal::open(&root.join("journal.sqlite"))?),
        Arc::new(SystemClock),
        Arc::new(RandomIds),
        vec![7; 32],
        root.join("attachments"),
    )?)
}

fn input(path: &Path) -> MailSendInput {
    MailSendInput {
        account_id: "work".into(),
        to: vec!["self@example.invalid".into()],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: "attachment fixture".into(),
        body: "body fixture".into(),
        attachments: vec![OutgoingAttachmentInput {
            path: path.to_string_lossy().into(),
            filename: Some("report.bin".into()),
            content_type: Some("application/octet-stream".into()),
        }],
        idempotency_key: "00000000-0000-4000-8000-000000000001".into(),
    }
}

#[tokio::test]
async fn attached_send_replays_after_restart_and_conflicts_on_changed_bytes() -> anyhow::Result<()>
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("input.bin");
    fs::write(&path, [0, 255, 1, 254])?;
    let backend = Arc::new(FakeBackend::new("work"));
    let first = runtime(backend.clone(), directory.path())?;
    let input = input(&path);
    assert_eq!(
        first.mail_send(input.clone()).await.data.map(|r| r.status),
        Some(OperationState::Succeeded)
    );
    let messages = backend.outgoing_messages()?;
    assert_eq!(
        messages.first().and_then(|m| m.attachments.first()).map(|a| a.bytes.as_slice()),
        Some([0, 255, 1, 254].as_slice())
    );
    drop(first);
    let restarted = runtime(backend.clone(), directory.path())?;
    assert_eq!(
        restarted.mail_send(input.clone()).await.data.map(|r| r.status),
        Some(OperationState::Succeeded)
    );
    assert_eq!(backend.operations()?.len(), 1);
    fs::write(&path, [9, 8, 7, 6])?;
    assert_eq!(
        restarted.mail_send(input).await.error.map(|e| e.code),
        Some(ErrorCode::IdempotencyConflict)
    );
    assert_eq!(backend.operations()?.len(), 1);
    let journal = SqliteJournal::open(&directory.path().join("journal.sqlite"))?;
    assert_eq!(journal.list(&Default::default())?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn reply_and_forward_pass_prepared_attachments_to_backend() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("input.bin");
    fs::write(&path, [0, 255, 1, 254])?;
    let backend = Arc::new(FakeBackend::new("work"));
    let runtime = runtime(backend.clone(), directory.path())?;
    let mail_ref = runtime
        .mail_list(MailListInput::default())
        .await
        .data
        .and_then(|p| p.items.into_iter().next())
        .map(|m| m.mail_ref)
        .ok_or_else(|| anyhow::anyhow!("missing mail fixture"))?;
    let attachments = input(&path).attachments;
    let reply = runtime
        .mail_reply(MailReplyInput {
            mail_ref: mail_ref.clone(),
            body: "reply".into(),
            reply_all: false,
            attachments: attachments.clone(),
            idempotency_key: "00000000-0000-4000-8000-000000000002".into(),
        })
        .await;
    assert_eq!(reply.data.map(|r| r.status), Some(OperationState::Succeeded));
    let forward = runtime
        .mail_forward(MailForwardInput {
            mail_ref,
            to: vec!["self@example.invalid".into()],
            cc: Vec::new(),
            bcc: Vec::new(),
            body: "forward".into(),
            attachments,
            idempotency_key: "00000000-0000-4000-8000-000000000003".into(),
        })
        .await;
    assert_eq!(forward.data.map(|r| r.status), Some(OperationState::Succeeded));
    assert_eq!(backend.outgoing_messages()?.len(), 2);
    for message in backend.outgoing_messages()? {
        assert_eq!(
            message.attachments.first().map(|a| a.bytes.as_slice()),
            Some([0, 255, 1, 254].as_slice())
        );
    }
    Ok(())
}

#[tokio::test]
async fn invalid_attachment_creates_no_journal_or_backend_mutation() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let backend = Arc::new(FakeBackend::new("work"));
    let runtime = runtime(backend.clone(), directory.path())?;
    let response = runtime.mail_send(input(&directory.path().join("missing"))).await;
    assert_eq!(response.error.map(|e| e.code), Some(ErrorCode::ValidationFailed));
    assert!(backend.operations()?.is_empty());
    let journal = SqliteJournal::open(&directory.path().join("journal.sqlite"))?;
    assert!(journal.list(&Default::default())?.is_empty());
    Ok(())
}
