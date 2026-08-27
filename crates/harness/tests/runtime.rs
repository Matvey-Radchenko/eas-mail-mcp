use std::sync::Arc;

use chrono::{Duration, TimeZone as _, Utc};
use eas_mail_mcp::backend::AccountBackend;
use eas_mail_mcp::{
    AttachmentDownloadInput, Clock, ErrorCode, IdGenerator, MailAttachmentsInput, MailGetInput,
    MailListInput, MailSendInput, OperationJournal, Runtime,
};
use eas_mail_mcp_harness::{FakeBackend, ManualClock, MemoryJournal, SequenceIds};

#[tokio::test]
async fn cursor_expires_after_fifteen_minutes() -> anyhow::Result<()> {
    let clock = ManualClock::new(timestamp()?);
    let backend = Arc::new(FakeBackend::new("work").with_mail_count(2));
    let (runtime, _directory) = runtime(vec![backend], &clock)?;
    let first = runtime.mail_list(list_input(None)).await;
    let cursor = first
        .data
        .and_then(|page| page.next_cursor)
        .ok_or_else(|| anyhow::anyhow!("first page returned no cursor"))?;

    clock.advance(Duration::minutes(16));
    let expired = runtime.mail_list(list_input(Some(cursor))).await;
    anyhow::ensure!(expired.error.is_some_and(|error| error.code == ErrorCode::ReferenceExpired));
    Ok(())
}

#[tokio::test]
async fn only_thirty_two_cursor_snapshots_are_retained() -> anyhow::Result<()> {
    let clock = ManualClock::new(timestamp()?);
    let backend = Arc::new(FakeBackend::new("work").with_mail_count(2));
    let (runtime, _directory) = runtime(vec![backend], &clock)?;
    let mut first_cursor = None;
    for index in 0..33 {
        let cursor = runtime
            .mail_list(list_input(None))
            .await
            .data
            .and_then(|page| page.next_cursor)
            .ok_or_else(|| anyhow::anyhow!("list returned no cursor"))?;
        if index == 0 {
            first_cursor = Some(cursor);
        }
    }
    let expired = runtime.mail_list(list_input(first_cursor)).await;
    anyhow::ensure!(expired.error.is_some_and(|error| error.code == ErrorCode::ReferenceExpired));
    Ok(())
}

#[tokio::test]
async fn partial_account_failure_returns_data_and_warning() -> anyhow::Result<()> {
    let clock = ManualClock::new(timestamp()?);
    let healthy = Arc::new(FakeBackend::new("healthy"));
    let failing = Arc::new(FakeBackend::failing("offline"));
    let (runtime, _directory) = runtime(vec![healthy, failing], &clock)?;

    let response = runtime.mail_list(MailListInput::default()).await;
    anyhow::ensure!(response.error.is_none());
    anyhow::ensure!(response.data.is_some_and(|data| data.items.len() == 1));
    anyhow::ensure!(response.warnings.len() == 1);
    anyhow::ensure!(
        response.warnings.first().is_some_and(|warning| warning.account_id == "offline")
    );
    Ok(())
}

#[tokio::test]
async fn repeated_write_uses_one_backend_operation_and_conflicts_on_changed_payload()
-> anyhow::Result<()> {
    let clock = ManualClock::new(timestamp()?);
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(vec![backend.clone()], &clock)?;
    let input = send_input("first");

    let first = runtime.mail_send(input.clone()).await;
    let repeated = runtime.mail_send(input).await;
    anyhow::ensure!(first.error.is_none() && repeated.error.is_none());
    anyhow::ensure!(backend.operations()?.len() == 1);

    let conflict = runtime.mail_send(send_input("changed")).await;
    anyhow::ensure!(
        conflict.error.is_some_and(|error| error.code == ErrorCode::IdempotencyConflict)
    );
    anyhow::ensure!(backend.operations()?.len() == 1);
    Ok(())
}

#[tokio::test]
async fn attachment_download_uses_private_account_cache_and_plain_text_output() -> anyhow::Result<()>
{
    let clock = ManualClock::new(timestamp()?);
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(vec![backend], &clock)?;
    let page = runtime.mail_list(MailListInput::default()).await;
    let summary = page
        .data
        .and_then(|data| data.items.into_iter().next())
        .ok_or_else(|| anyhow::anyhow!("mail list is empty"))?;
    anyhow::ensure!(!summary.preview.contains('<'));
    anyhow::ensure!(summary.untrusted_external_content);
    let attachments =
        runtime.mail_list_attachments(MailAttachmentsInput { mail_ref: summary.mail_ref }).await;
    let reference = attachments
        .data
        .and_then(|data| data.attachments.into_iter().next())
        .ok_or_else(|| anyhow::anyhow!("attachment list is empty"))?;
    let downloaded = runtime
        .mail_download_attachment(AttachmentDownloadInput {
            attachment_ref: reference.attachment_ref,
        })
        .await
        .data
        .ok_or_else(|| anyhow::anyhow!("attachment download failed"))?;
    let downloaded_path = std::path::Path::new(&downloaded.path);
    anyhow::ensure!(
        downloaded_path.parent().and_then(std::path::Path::file_name)
            == Some(std::ffi::OsStr::new("work"))
    );
    anyhow::ensure!(
        downloaded_path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("file_"))
    );
    anyhow::ensure!(std::fs::read(&downloaded.path)? == b"attachment payload");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        anyhow::ensure!(std::fs::metadata(&downloaded.path)?.permissions().mode() & 0o777 == 0o600);
    }
    Ok(())
}

#[tokio::test]
async fn direct_read_remote_wipe_purges_references_files_and_journal() -> anyhow::Result<()> {
    let clock = ManualClock::new(timestamp()?);
    let wiped = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(vec![wiped.clone()], &clock)?;
    anyhow::ensure!(runtime.mail_send(send_input("before wipe")).await.error.is_none());

    let listed = runtime
        .mail_list(MailListInput {
            account_ids: Some(vec!["work".into()]),
            ..MailListInput::default()
        })
        .await;
    let mail = listed
        .data
        .and_then(|data| data.items.into_iter().next())
        .ok_or_else(|| anyhow::anyhow!("mail list is empty"))?;
    let mail_ref = mail.mail_ref.clone();
    let attachments =
        runtime.mail_list_attachments(MailAttachmentsInput { mail_ref: mail.mail_ref }).await;
    let attachment_ref = attachments
        .data
        .and_then(|data| data.attachments.into_iter().next())
        .map(|attachment| attachment.attachment_ref)
        .ok_or_else(|| anyhow::anyhow!("attachment list is empty"))?;
    let path = runtime
        .mail_download_attachment(AttachmentDownloadInput { attachment_ref })
        .await
        .data
        .map(|download| download.path)
        .ok_or_else(|| anyhow::anyhow!("attachment download failed"))?;

    wiped.set_failure(Some(ErrorCode::RemoteWipe))?;
    let wiped_read =
        runtime.mail_get(MailGetInput { mail_ref: mail_ref.clone(), body_limit: None }).await;
    anyhow::ensure!(wiped_read.error.is_some_and(|error| error.code == ErrorCode::RemoteWipe));
    anyhow::ensure!(!std::path::Path::new(&path).exists());
    let blocked = runtime.mail_get(MailGetInput { mail_ref, body_limit: None }).await;
    anyhow::ensure!(blocked.error.is_some_and(|error| error.code == ErrorCode::RemoteWipe));

    wiped.set_failure(None)?;
    let write = runtime.mail_send(send_input("after wipe")).await;
    anyhow::ensure!(write.error.is_some_and(|error| error.code == ErrorCode::RemoteWipe));
    anyhow::ensure!(wiped.operations()?.len() == 1);
    Ok(())
}

#[tokio::test]
async fn write_remote_wipe_removes_pending_idempotency_state() -> anyhow::Result<()> {
    let clock = ManualClock::new(timestamp()?);
    let wiped = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(vec![wiped.clone()], &clock)?;
    wiped.set_failure(Some(ErrorCode::RemoteWipe))?;

    let response = runtime.mail_send(send_input("wipe")).await;
    anyhow::ensure!(response.error.is_some_and(|error| error.code == ErrorCode::RemoteWipe));
    wiped.set_failure(None)?;
    let blocked = runtime.mail_send(send_input("wipe")).await;
    anyhow::ensure!(blocked.error.is_some_and(|error| error.code == ErrorCode::RemoteWipe));
    anyhow::ensure!(wiped.operations()?.is_empty());
    Ok(())
}

fn runtime(
    backends: Vec<Arc<FakeBackend>>,
    clock: &ManualClock,
) -> anyhow::Result<(Runtime, tempfile::TempDir)> {
    let directory = tempfile::tempdir()?;
    let boundaries =
        backends.into_iter().map(|backend| -> Arc<dyn AccountBackend> { backend }).collect();
    let journal: Arc<dyn OperationJournal> = Arc::new(MemoryJournal::default());
    let clock_boundary: Arc<dyn Clock> = Arc::new(clock.clone());
    let ids: Arc<dyn IdGenerator> = Arc::new(SequenceIds::default());
    let runtime = Runtime::with_dependencies(
        boundaries,
        journal,
        clock_boundary,
        ids,
        vec![7; 32],
        directory.path().join("attachments"),
    )?;
    Ok((runtime, directory))
}

fn list_input(cursor: Option<String>) -> MailListInput {
    MailListInput { cursor, limit: Some(1), ..MailListInput::default() }
}

fn send_input(subject: &str) -> MailSendInput {
    MailSendInput {
        account_id: "work".into(),
        to: vec!["self@example.invalid".into()],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: subject.into(),
        body: "fixture body".into(),
        idempotency_key: "11111111-2222-4333-8444-555555555555".into(),
    }
}

fn timestamp() -> anyhow::Result<chrono::DateTime<Utc>> {
    Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("invalid fixture timestamp"))
}
