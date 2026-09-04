use std::sync::{Arc, Barrier};

use eas_mail_mcp::{
    ErrorCode, JournalFilter, MailResultLocator, OperationJournal as _, OperationStatus,
    SqliteJournal,
};

use super::record;

#[test]
fn unresolved_rows_survive_retention_while_completed_rows_expire() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.sqlite");
    let journal = SqliteJournal::open(&path)?;
    let states = [
        OperationStatus::Pending,
        OperationStatus::Unknown,
        OperationStatus::Partial,
        OperationStatus::Succeeded,
        OperationStatus::Failed,
    ];
    for (index, status) in states.into_iter().enumerate() {
        let mut input = record("work", "fingerprint");
        input.operation_id = format!("11111111-2222-4333-8444-{index:012}");
        journal.begin(&input)?;
        if status != OperationStatus::Pending {
            journal.finish(&input.operation_id, status, 1)?;
        }
    }
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute("UPDATE operations SET updated_at=unixepoch()-7776001", [])?;
    assert_eq!(journal.prune()?, 2);
    let remaining = journal.list(&JournalFilter::default())?;
    assert_eq!(remaining.len(), 3);
    for state in [OperationStatus::Pending, OperationStatus::Unknown, OperationStatus::Partial] {
        assert!(remaining.iter().any(|entry| entry.record.status == state));
    }
    Ok(())
}

#[test]
fn inspection_filters_bounds_and_timestamps_do_not_mutate_rows() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.sqlite");
    let journal = SqliteJournal::open(&path)?;
    let first = record("first", "fingerprint-a");
    let mut second = record("second", "fingerprint-b");
    second.operation_id = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into();
    journal.begin(&first)?;
    journal.begin(&second)?;
    journal.finish(&second.operation_id, OperationStatus::Unknown, 2)?;
    let connection = rusqlite::Connection::open(&path)?;
    connection
        .execute("UPDATE operations SET updated_at=updated_at+10 WHERE account_id='second'", [])?;
    let before =
        journal.inspect(&first.operation_id)?.ok_or_else(|| anyhow::anyhow!("missing row"))?;
    assert!(before.created_at > 0);
    assert!(before.updated_at >= before.created_at);
    let latest = journal.list(&JournalFilter { limit: 1, ..Default::default() })?;
    assert_eq!(latest.len(), 1);
    assert_eq!(latest.first().map(|entry| &entry.record.operation_id), Some(&second.operation_id));
    let pending = journal.list(&JournalFilter {
        account_id: Some("first".into()),
        status: Some(OperationStatus::Pending),
        limit: 10,
    })?;
    assert_eq!(pending, vec![before.clone()]);
    assert_eq!(journal.inspect(&first.operation_id)?, Some(before));
    assert!(journal.inspect("missing")?.is_none());
    for limit in [0, 101] {
        assert!(journal.list(&JournalFilter { limit, ..Default::default() }).is_err());
    }
    Ok(())
}

#[test]
fn confirmed_move_locator_and_original_fingerprint_survive_reopen() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.sqlite");
    let input = record("work", "stable-input-hmac");
    let locator =
        MailResultLocator { folder_id: "deleted-items".into(), server_id: "4:123".into() };
    {
        let journal = SqliteJournal::open(&path)?;
        journal.begin(&input)?;
        journal.finish_with_locator(
            &input.operation_id,
            OperationStatus::Succeeded,
            0,
            Some(&locator),
        )?;
    }
    let reopened = SqliteJournal::open(&path)?;
    let replay = reopened.begin(&input)?;
    assert!(!replay.inserted);
    assert_eq!(replay.record.status, OperationStatus::Succeeded);
    assert_eq!(replay.record.payload_hmac, "stable-input-hmac");
    let entry =
        reopened.inspect(&input.operation_id)?.ok_or_else(|| anyhow::anyhow!("missing row"))?;
    assert_eq!(entry.result_locator, Some(locator));
    Ok(())
}

#[test]
fn invalid_move_locator_cannot_change_journal_state() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let journal = SqliteJournal::open(&directory.path().join("journal.sqlite"))?;
    let input = record("work", "fingerprint");
    journal.begin(&input)?;
    for folder in [String::new(), "bad\nfolder".into(), "a".repeat(8193)] {
        let locator = MailResultLocator { folder_id: folder, server_id: "id".into() };
        let result = journal.finish_with_locator(
            &input.operation_id,
            OperationStatus::Succeeded,
            0,
            Some(&locator),
        );
        assert!(result.is_err_and(|error| error.envelope.code == ErrorCode::ValidationFailed));
        assert_eq!(
            journal.lookup(&input.operation_id)?.map(|row| row.status),
            Some(OperationStatus::Pending)
        );
    }
    let locator = MailResultLocator { folder_id: "folder".into(), server_id: "id".into() };
    assert!(
        journal
            .finish_with_locator(&input.operation_id, OperationStatus::Unknown, 0, Some(&locator))
            .is_err()
    );
    Ok(())
}

#[test]
fn future_schema_is_rejected_without_reclassifying_existing_operations() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.sqlite");
    let journal = SqliteJournal::open(&path)?;
    let input = record("work", "fingerprint");
    journal.begin(&input)?;
    let connection = rusqlite::Connection::open(&path)?;
    connection.pragma_update(None, "user_version", 999)?;
    assert!(
        SqliteJournal::open(&path)
            .is_err_and(|error| error.envelope.message.contains("newer application"))
    );
    assert_eq!(
        journal.lookup(&input.operation_id)?.map(|row| row.status),
        Some(OperationStatus::Pending)
    );
    let version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    assert_eq!(version, 999);
    Ok(())
}

#[test]
fn failed_legacy_migration_rolls_back_added_columns_and_version() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.sqlite");
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch("CREATE TABLE operations (operation_id TEXT PRIMARY KEY)")?;
    assert!(SqliteJournal::open(&path).is_err());
    let version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    assert_eq!(version, 0);
    let mut columns = connection.prepare("PRAGMA table_info(operations)")?;
    let columns = columns
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    assert_eq!(columns, ["operation_id"]);
    Ok(())
}

#[test]
fn simultaneous_legacy_upgrades_preserve_operation_metadata() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.sqlite");
    {
        let connection = rusqlite::Connection::open(&path)?;
        connection.execute_batch(
            "CREATE TABLE operations (
            operation_id TEXT PRIMARY KEY, account_id TEXT NOT NULL, kind TEXT NOT NULL,
            payload_hmac TEXT NOT NULL, client_id TEXT NOT NULL, status TEXT NOT NULL,
            created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
            INSERT INTO operations VALUES ('operation', 'work', 'mail_send', 'fingerprint',
            'client', 'pending', 1234, 1234);",
        )?;
    }
    let barrier = Arc::new(Barrier::new(8));
    let threads = (0..8)
        .map(|_| {
            let barrier = Arc::clone(&barrier);
            let path = path.clone();
            std::thread::spawn(move || -> anyhow::Result<()> {
                barrier.wait();
                let journal = SqliteJournal::open(&path)?;
                let entry = journal
                    .inspect("operation")?
                    .ok_or_else(|| anyhow::anyhow!("missing legacy row"))?;
                assert_eq!(entry.record.status, OperationStatus::Pending);
                assert_eq!(entry.record.payload_hmac, "fingerprint");
                assert_eq!(entry.created_at, 1234);
                assert_eq!(entry.updated_at, 1234);
                assert_eq!(entry.result_locator, None);
                Ok(())
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().map_err(|_| anyhow::anyhow!("upgrade worker failed"))??;
    }
    Ok(())
}
