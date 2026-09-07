use eas_mail_mcp::{
    ErrorCode, JournalRecord, OperationJournal as _, OperationStatus, SqliteJournal,
};

#[test]
fn opening_journal_preserves_pending_until_explicit_account_recovery() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("operations.sqlite");
    let record = record("work", "fingerprint-a");
    {
        let journal = SqliteJournal::open(&path)?;
        let inserted = journal.begin(&record)?;
        anyhow::ensure!(inserted.inserted);
    }
    let journal = SqliteJournal::open(&path)?;
    anyhow::ensure!(
        journal
            .lookup(&record.operation_id)?
            .is_some_and(|row| row.status == OperationStatus::Pending)
    );
    anyhow::ensure!(journal.pending_accounts()? == ["work"]);
    anyhow::ensure!(journal.recover_account("other")? == 0);
    anyhow::ensure!(journal.recover_account("work")? == 1);
    let existing = journal.begin(&record)?;
    anyhow::ensure!(!existing.inserted);
    anyhow::ensure!(existing.record.status == OperationStatus::Unknown);
    Ok(())
}

#[test]
fn idempotency_key_rejects_changed_payload_and_missing_finish() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let journal = SqliteJournal::open(&directory.path().join("operations.sqlite"))?;
    let first = record("work", "fingerprint-a");
    let mut changed = first.clone();
    changed.payload_hmac = "fingerprint-b".into();
    let _ = journal.begin(&first)?;
    let conflict = journal.begin(&changed);
    anyhow::ensure!(
        conflict.err().is_some_and(|error| error.envelope.code == ErrorCode::IdempotencyConflict)
    );
    anyhow::ensure!(journal.finish("missing-operation", OperationStatus::Succeeded, 0).is_err());
    Ok(())
}

#[test]
fn partial_checkpoints_survive_reopen_without_calendar_content() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("operations.sqlite");
    let record = record("work", "fingerprint-a");
    {
        let journal = SqliteJournal::open(&path)?;
        let _ = journal.begin(&record)?;
        journal.checkpoint(&record.operation_id, 5)?;
        journal.finish(&record.operation_id, OperationStatus::Partial, 5)?;
    }
    let reopened = SqliteJournal::open(&path)?;
    let stored = reopened
        .lookup(&record.operation_id)?
        .ok_or_else(|| anyhow::anyhow!("partial journal row is missing"))?;
    anyhow::ensure!(stored.status == OperationStatus::Partial);
    anyhow::ensure!(stored.completed_steps == 5);
    Ok(())
}

#[test]
fn legacy_journal_migrates_completed_steps_atomically() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("operations.sqlite");
    {
        let connection = rusqlite::Connection::open(&path)?;
        connection.execute_batch(
            "CREATE TABLE operations (
               operation_id TEXT PRIMARY KEY,
               account_id TEXT NOT NULL,
               kind TEXT NOT NULL,
               payload_hmac TEXT NOT NULL,
               client_id TEXT NOT NULL,
               status TEXT NOT NULL,
               created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL
             );
             INSERT INTO operations VALUES (
               '11111111-2222-4333-8444-555555555555', 'work', 'mail_send',
               'fingerprint-a', 'client', 'pending', unixepoch(), unixepoch()
             );",
        )?;
    }
    let journal = SqliteJournal::open(&path)?;
    let stored = journal
        .lookup("11111111-2222-4333-8444-555555555555")?
        .ok_or_else(|| anyhow::anyhow!("migrated journal row is missing"))?;
    anyhow::ensure!(stored.status == OperationStatus::Pending);
    anyhow::ensure!(stored.completed_steps == 0);
    Ok(())
}

#[test]
fn account_purge_and_retention_remove_only_metadata() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("operations.sqlite");
    let journal = SqliteJournal::open(&path)?;
    let work = record("work", "fingerprint-a");
    let mut other = record("other", "fingerprint-b");
    other.operation_id = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into();
    let _ = journal.begin(&work)?;
    let _ = journal.begin(&other)?;
    journal.finish(&work.operation_id, OperationStatus::Succeeded, 0)?;
    journal.finish(&other.operation_id, OperationStatus::Succeeded, 0)?;
    anyhow::ensure!(journal.purge_account("work")? == 1);

    let connection = rusqlite::Connection::open(&path)?;
    let schema: String = connection.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='operations'",
        [],
        |row| row.get(0),
    )?;
    anyhow::ensure!(!schema.to_ascii_lowercase().contains("body"));
    anyhow::ensure!(!schema.to_ascii_lowercase().contains("recipient"));
    connection.execute(
        "UPDATE operations SET updated_at=unixepoch() - 7776001 WHERE operation_id=?1",
        [&other.operation_id],
    )?;
    drop(connection);
    anyhow::ensure!(journal.prune()? == 1);
    Ok(())
}

fn record(account_id: &str, fingerprint: &str) -> JournalRecord {
    JournalRecord {
        operation_id: "11111111-2222-4333-8444-555555555555".into(),
        account_id: account_id.into(),
        kind: "mail_send".into(),
        payload_hmac: fingerprint.into(),
        client_id: "11111111-2222-4333-8444-555555555555".into(),
        status: OperationStatus::Pending,
        completed_steps: 0,
    }
}

#[path = "journal/reliability.rs"]
mod reliability;
