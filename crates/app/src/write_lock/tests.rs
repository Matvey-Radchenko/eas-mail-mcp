use std::time::Duration;

use super::*;

#[tokio::test]
async fn independent_lock_instances_serialize_one_account() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let first = WriteLocks::new(directory.path().join("locks"))?;
    let second = WriteLocks::new(directory.path().join("locks"))?;
    let guard = first.acquire("work").await?;
    let waiting = tokio::spawn(async move { second.acquire("work").await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!waiting.is_finished());
    drop(guard);
    let second_guard = tokio::time::timeout(Duration::from_secs(1), waiting)
        .await
        .map_err(|_| anyhow::anyhow!("second lock did not unblock"))??;
    drop(second_guard?);
    Ok(())
}

#[tokio::test]
async fn different_accounts_do_not_block_each_other() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let locks = WriteLocks::new(directory.path().join("locks"))?;
    let first = locks.acquire("first").await?;
    let second = tokio::time::timeout(Duration::from_secs(1), locks.acquire("second")).await??;
    drop((first, second));
    Ok(())
}

#[tokio::test]
async fn lock_timeout_and_cancel_leave_no_queued_worker() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let locks = std::sync::Arc::new(WriteLocks::new(directory.path().join("locks"))?);
    let guard = locks.acquire("work").await?;
    assert!(locks.try_acquire("work")?.is_none());
    let timeout = locks.acquire_with_timeout("work", Duration::from_millis(35)).await;
    assert!(timeout.is_err_and(|error| error.envelope.code == ErrorCode::StorageError));
    let waiter = std::sync::Arc::clone(&locks);
    let task = tokio::spawn(async move { waiter.acquire("work").await });
    tokio::time::sleep(Duration::from_millis(10)).await;
    task.abort();
    assert!(task.await.is_err_and(|error| error.is_cancelled()));
    drop(guard);
    for _ in 0..3 {
        assert!(locks.try_acquire("work")?.is_some());
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    Ok(())
}

#[tokio::test]
async fn runtime_recovery_does_not_relabel_an_active_writer() -> anyhow::Result<()> {
    use crate::{
        JournalRecord, OperationJournal as _, OperationStatus, RandomIds, Runtime, SqliteJournal,
        SystemClock,
    };
    use std::sync::Arc;

    let directory = tempfile::tempdir()?;
    let locks = WriteLocks::new(directory.path().join("write-locks"))?;
    let guard = locks.acquire("work").await?;
    let path = directory.path().join("journal.sqlite");
    let first = SqliteJournal::open(&path)?;
    let record = JournalRecord {
        operation_id: "11111111-2222-4333-8444-555555555555".into(),
        account_id: "work".into(),
        kind: "calendar_create".into(),
        payload_hmac: "hash".into(),
        client_id: "client".into(),
        status: OperationStatus::Pending,
        completed_steps: 0,
    };
    first.begin(&record)?;
    let second = Arc::new(SqliteJournal::open(&path)?);
    let runtime = Runtime::with_dependencies(
        Vec::new(),
        second.clone(),
        Arc::new(SystemClock),
        Arc::new(RandomIds),
        vec![0; 32],
        directory.path().join("attachments"),
    )?;
    assert_eq!(
        second.lookup(&record.operation_id)?.map(|r| r.status),
        Some(OperationStatus::Pending)
    );
    first.checkpoint(&record.operation_id, 1)?;
    drop(runtime);
    drop(guard);
    let _restarted = Runtime::with_dependencies(
        Vec::new(),
        second.clone(),
        Arc::new(SystemClock),
        Arc::new(RandomIds),
        vec![0; 32],
        directory.path().join("attachments"),
    )?;
    let recovered =
        second.lookup(&record.operation_id)?.ok_or_else(|| anyhow::anyhow!("missing row"))?;
    assert_eq!(recovered.status, OperationStatus::Unknown);
    assert_eq!(recovered.completed_steps, 1);
    Ok(())
}
