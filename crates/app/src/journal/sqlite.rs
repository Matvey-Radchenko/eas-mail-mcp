use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};

use super::{
    JournalBegin, JournalEntry, JournalFilter, JournalRecord, MailResultLocator, OperationJournal,
    OperationStatus, schema, storage_error,
};
use crate::{AppError, ErrorCode, Result, platform};

const SELECT: &str = "SELECT operation_id, account_id, kind, payload_hmac, client_id, status,
                     completed_steps, created_at, updated_at, result_locator FROM operations";

/// SQLite WAL journal safe for independent stdio processes.
pub struct SqliteJournal {
    connection: Mutex<Connection>,
}

impl SqliteJournal {
    /// Opens and transactionally migrates the journal without altering active operations.
    ///
    /// Orphan recovery is a separate operation that requires an account write lock.
    pub fn open(path: &Path) -> Result<Self> {
        let mut connection = private_connection(path)?;
        configure_wal(&connection)?;
        schema::migrate(&mut connection)?;
        Ok(Self { connection: Mutex::new(connection) })
    }
}

// SQLite may return SQLITE_BUSY while changing journal mode without invoking its busy handler.
// Retry only these idempotent connection settings, within the same bounded startup budget.
fn configure_wal(connection: &Connection) -> Result<()> {
    let wait_limit = Duration::from_secs(5);
    let deadline = Instant::now() + wait_limit;
    connection.busy_timeout(Duration::ZERO).map_err(|_| storage_error())?;
    loop {
        match connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;") {
            Ok(()) => return connection.busy_timeout(wait_limit).map_err(|_| storage_error()),
            Err(error)
                if matches!(
                    error.sqlite_error_code(),
                    Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
                ) =>
            {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(storage_error());
                }
                std::thread::sleep(Duration::from_millis(25).min(remaining));
            }
            Err(_) => return Err(storage_error()),
        }
    }
}

pub(crate) fn with_storage_write_lock<T>(
    path: &Path,
    action: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let mut connection = private_connection(path)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| storage_error())?;
    let output = action()?;
    transaction.commit().map_err(|_| storage_error())?;
    Ok(output)
}

fn private_connection(path: &Path) -> Result<Connection> {
    let parent = path.parent().ok_or_else(storage_error)?;
    platform::ensure_private_directory(parent).map_err(|_| storage_error())?;
    platform::open_private_append(path).map_err(|_| storage_error())?;
    let connection = Connection::open(path).map_err(|_| storage_error())?;
    connection.busy_timeout(Duration::from_secs(5)).map_err(|_| storage_error())?;
    Ok(connection)
}

impl OperationJournal for SqliteJournal {
    fn lookup(&self, operation_id: &str) -> Result<Option<JournalRecord>> {
        self.inspect(operation_id).map(|entry| entry.map(|entry| entry.record))
    }

    fn inspect(&self, operation_id: &str) -> Result<Option<JournalEntry>> {
        self.connection
            .lock()
            .map_err(|_| storage_error())?
            .query_row(&format!("{SELECT} WHERE operation_id=?1"), [operation_id], row_to_entry)
            .optional()
            .map_err(|_| storage_error())
    }

    fn list(&self, filter: &JournalFilter) -> Result<Vec<JournalEntry>> {
        filter.validate()?;
        let connection = self.connection.lock().map_err(|_| storage_error())?;
        let mut statement = connection
            .prepare(&format!(
                "{SELECT} WHERE (?1 IS NULL OR account_id=?1) AND (?2 IS NULL OR status=?2)
             ORDER BY updated_at DESC, operation_id ASC LIMIT ?3",
            ))
            .map_err(|_| storage_error())?;
        let values = statement
            .query_map(
                params![
                    filter.account_id,
                    filter.status.map(OperationStatus::as_str),
                    filter.limit
                ],
                row_to_entry,
            )
            .map_err(|_| storage_error())?;
        values.collect::<rusqlite::Result<Vec<_>>>().map_err(|_| storage_error())
    }

    fn pending_accounts(&self) -> Result<Vec<String>> {
        let connection = self.connection.lock().map_err(|_| storage_error())?;
        let mut statement = connection.prepare(
            "SELECT DISTINCT account_id FROM operations WHERE status='pending' ORDER BY account_id",
        ).map_err(|_| storage_error())?;
        statement
            .query_map([], |row| row.get(0))
            .map_err(|_| storage_error())?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|_| storage_error())
    }

    fn recover_account(&self, account_id: &str) -> Result<usize> {
        self.connection
            .lock()
            .map_err(|_| storage_error())?
            .execute(
                "UPDATE operations SET status='unknown', updated_at=unixepoch()
             WHERE account_id=?1 AND status='pending'",
                [account_id],
            )
            .map_err(|_| storage_error())
    }

    fn begin(&self, record: &JournalRecord) -> Result<JournalBegin> {
        let mut connection = self.connection.lock().map_err(|_| storage_error())?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| storage_error())?;
        let existing = transaction
            .query_row(
                &format!("{SELECT} WHERE operation_id=?1"),
                [&record.operation_id],
                row_to_entry,
            )
            .optional()
            .map_err(|_| storage_error())?;
        if let Some(existing) = existing {
            let existing = existing.record;
            if existing.account_id != record.account_id
                || existing.kind != record.kind
                || existing.payload_hmac != record.payload_hmac
            {
                return Err(AppError::new(
                    ErrorCode::IdempotencyConflict,
                    "idempotency key was already used for different input",
                ));
            }
            transaction.commit().map_err(|_| storage_error())?;
            return Ok(JournalBegin { record: existing, inserted: false });
        }
        transaction
            .execute(
                "INSERT INTO operations (operation_id, account_id, kind, payload_hmac, client_id,
             status, completed_steps, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, unixepoch(), unixepoch())",
                params![
                    record.operation_id,
                    record.account_id,
                    record.kind,
                    record.payload_hmac,
                    record.client_id,
                    record.completed_steps
                ],
            )
            .map_err(|_| storage_error())?;
        transaction.commit().map_err(|_| storage_error())?;
        Ok(JournalBegin { record: record.clone(), inserted: true })
    }

    fn checkpoint(&self, operation_id: &str, completed_steps: u32) -> Result<()> {
        let updated = self
            .connection
            .lock()
            .map_err(|_| storage_error())?
            .execute(
                "UPDATE operations SET completed_steps=?1, updated_at=unixepoch()
             WHERE operation_id=?2 AND status='pending'",
                params![completed_steps, operation_id],
            )
            .map_err(|_| storage_error())?;
        require_updated(updated)
    }

    fn finish_with_locator(
        &self,
        operation_id: &str,
        status: OperationStatus,
        completed_steps: u32,
        locator: Option<&MailResultLocator>,
    ) -> Result<()> {
        let encoded = encode_locator(status, locator)?;
        let updated = self
            .connection
            .lock()
            .map_err(|_| storage_error())?
            .execute(
                "UPDATE operations SET status=?1, completed_steps=?2, result_locator=?3,
             updated_at=unixepoch() WHERE operation_id=?4",
                params![status.as_str(), completed_steps, encoded, operation_id],
            )
            .map_err(|_| storage_error())?;
        require_updated(updated)
    }

    fn prune(&self) -> Result<usize> {
        self.connection
            .lock()
            .map_err(|_| storage_error())?
            .execute(
                "DELETE FROM operations WHERE status IN ('succeeded', 'failed')
             AND updated_at < unixepoch() - 7776000",
                [],
            )
            .map_err(|_| storage_error())
    }

    fn purge_account(&self, account_id: &str) -> Result<usize> {
        self.connection
            .lock()
            .map_err(|_| storage_error())?
            .execute("DELETE FROM operations WHERE account_id=?1", [account_id])
            .map_err(|_| storage_error())
    }
}

fn require_updated(count: usize) -> Result<()> {
    if count == 1 { Ok(()) } else { Err(storage_error()) }
}

fn encode_locator(
    status: OperationStatus,
    locator: Option<&MailResultLocator>,
) -> Result<Option<String>> {
    if status == OperationStatus::Pending {
        return Err(AppError::new(
            ErrorCode::StorageError,
            "cannot finish an operation as pending",
        ));
    }
    if locator.is_some() && status != OperationStatus::Succeeded {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "a move result locator requires confirmed success",
        ));
    }
    locator
        .map(|locator| {
            locator.validate()?;
            serde_json::to_string(locator).map_err(|_| storage_error())
        })
        .transpose()
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<JournalEntry> {
    let status: String = row.get(5)?;
    let status = OperationStatus::parse(&status).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let locator: Option<String> = row.get(9)?;
    let result_locator = locator
        .map(|text| {
            if text.len() > 100_000 {
                return Err(rusqlite::Error::InvalidQuery);
            }
            let locator: MailResultLocator =
                serde_json::from_str(&text).map_err(|_| rusqlite::Error::InvalidQuery)?;
            locator.validate().map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(locator)
        })
        .transpose()?;
    Ok(JournalEntry {
        record: JournalRecord {
            operation_id: row.get(0)?,
            account_id: row.get(1)?,
            kind: row.get(2)?,
            payload_hmac: row.get(3)?,
            client_id: row.get(4)?,
            status,
            completed_steps: row.get(6)?,
        },
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        result_locator,
    })
}
