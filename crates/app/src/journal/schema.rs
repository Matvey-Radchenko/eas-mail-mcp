use rusqlite::{Connection, TransactionBehavior};

use super::storage_error;
use crate::{AppError, ErrorCode, Result};

pub(super) const VERSION: u32 = 1;

pub(super) fn migrate(connection: &mut Connection) -> Result<()> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| storage_error())?;
    let version: u32 = transaction
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|_| storage_error())?;
    if version > VERSION {
        return Err(AppError::new(
            ErrorCode::StorageError,
            "operation journal requires a newer application; upgrade instead of downgrading",
        ));
    }
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS operations (
           operation_id TEXT PRIMARY KEY,
           account_id TEXT NOT NULL,
           kind TEXT NOT NULL,
           payload_hmac TEXT NOT NULL,
           client_id TEXT NOT NULL,
           status TEXT NOT NULL,
           completed_steps INTEGER NOT NULL DEFAULT 0,
           created_at INTEGER NOT NULL,
           updated_at INTEGER NOT NULL,
           result_locator TEXT
         );",
        )
        .map_err(|_| storage_error())?;
    if version == 0 {
        let columns = columns(&transaction)?;
        if !columns.iter().any(|name| name == "completed_steps") {
            transaction
                .execute_batch(
                    "ALTER TABLE operations ADD COLUMN completed_steps INTEGER NOT NULL DEFAULT 0",
                )
                .map_err(|_| storage_error())?;
        }
        if !columns.iter().any(|name| name == "result_locator") {
            transaction
                .execute_batch("ALTER TABLE operations ADD COLUMN result_locator TEXT")
                .map_err(|_| storage_error())?;
        }
        transaction.pragma_update(None, "user_version", VERSION).map_err(|_| storage_error())?;
    }
    validate_columns(&transaction)?;
    transaction
        .execute_batch(
            "CREATE INDEX IF NOT EXISTS operations_account_updated
         ON operations(account_id, updated_at DESC);
         CREATE INDEX IF NOT EXISTS operations_status_updated
         ON operations(status, updated_at DESC);",
        )
        .map_err(|_| storage_error())?;
    transaction.commit().map_err(|_| storage_error())
}

fn columns(connection: &Connection) -> Result<Vec<String>> {
    let mut statement =
        connection.prepare("PRAGMA table_info(operations)").map_err(|_| storage_error())?;
    statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|_| storage_error())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| storage_error())
}

fn validate_columns(connection: &Connection) -> Result<()> {
    let actual = columns(connection)?;
    if [
        "operation_id",
        "account_id",
        "kind",
        "payload_hmac",
        "client_id",
        "status",
        "completed_steps",
        "created_at",
        "updated_at",
        "result_locator",
    ]
    .iter()
    .all(|required| actual.iter().any(|column| column == required))
    {
        Ok(())
    } else {
        Err(storage_error())
    }
}
