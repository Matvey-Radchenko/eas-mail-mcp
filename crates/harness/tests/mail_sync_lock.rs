use std::fs::OpenOptions;
use std::sync::Arc;
use std::time::Duration;

use eas_mail_mcp::{AccountSelection, MailListInput, Runtime};
use eas_mail_mcp_harness::{FakeBackend, FixedClock, MemoryJournal, SequenceIds};

#[tokio::test]
async fn explicit_mail_sync_waits_for_account_owner_and_can_be_cancelled() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let now = chrono::DateTime::from_timestamp(1_700_000_000, 0)
        .ok_or_else(|| anyhow::anyhow!("fixture time"))?;
    let runtime = Runtime::with_dependencies(
        vec![Arc::new(FakeBackend::new("first")), Arc::new(FakeBackend::new("second"))],
        Arc::new(MemoryJournal::default()),
        Arc::new(FixedClock::new(now)),
        Arc::new(SequenceIds::default()),
        vec![7; 32],
        directory.path().join("attachments"),
    )?;
    let owner = OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.path().join("write-locks/first.lock"))?;
    owner.lock()?;
    let first = AccountSelection { account_ids: Some(vec!["first".into()]) };
    let blocked =
        tokio::time::timeout(Duration::from_millis(75), runtime.sync_now(first.clone())).await;
    assert!(blocked.is_err(), "Sync must not invalidate bindings while a writer owns the account");
    let blocked_list = tokio::time::timeout(
        Duration::from_millis(75),
        runtime.mail_list(MailListInput {
            account_ids: first.account_ids.clone(),
            ..Default::default()
        }),
    )
    .await;
    assert!(blocked_list.is_err(), "mail_list must use the same account lock");
    let other = tokio::time::timeout(
        Duration::from_secs(1),
        runtime.mail_list(MailListInput {
            account_ids: Some(vec!["second".into()]),
            ..Default::default()
        }),
    )
    .await?;
    assert!(other.error.is_none(), "another account remains independently readable");
    drop(owner);
    let status =
        runtime.sync_status(first.clone()).data.ok_or_else(|| anyhow::anyhow!("status"))?;
    assert!(status.reports.is_empty(), "cancelled Sync did not later execute");
    let completed = tokio::time::timeout(Duration::from_secs(1), runtime.sync_now(first)).await?;
    assert!(completed.error.is_none(), "explicit Sync works after the owner releases its lock");
    Ok(())
}
