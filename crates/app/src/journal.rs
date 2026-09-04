mod model;
mod schema;
mod sqlite;

use hmac::{Hmac, Mac as _};
use sha2::Sha256;

use crate::{AppError, ErrorCode, Result};

pub use model::{
    JournalBegin, JournalEntry, JournalFilter, JournalRecord, MailResultLocator, OperationStatus,
};
pub use sqlite::SqliteJournal;
pub(crate) use sqlite::with_storage_write_lock;

/// I/O boundary for idempotent external mutations.
pub trait OperationJournal: Send + Sync {
    /// Returns one existing operation without changing its state.
    fn lookup(&self, operation_id: &str) -> Result<Option<JournalRecord>>;
    /// Returns inspectable metadata without changing operation state.
    fn inspect(&self, operation_id: &str) -> Result<Option<JournalEntry>>;
    /// Lists bounded metadata, newest first, without changing operation state.
    fn list(&self, filter: &JournalFilter) -> Result<Vec<JournalEntry>>;
    /// Returns accounts with pending rows; their owners may still be active.
    fn pending_accounts(&self) -> Result<Vec<String>>;
    /// Marks orphaned pending operations unknown for one account.
    ///
    /// The caller must hold the account's exclusive write lock throughout this call.
    fn recover_account(&self, account_id: &str) -> Result<usize>;
    /// Inserts a pending row or returns an existing matching row.
    fn begin(&self, record: &JournalRecord) -> Result<JournalBegin>;
    /// Persists confirmed completed steps while an operation remains pending.
    fn checkpoint(&self, operation_id: &str, completed_steps: u32) -> Result<()>;
    /// Changes durable operation state.
    fn finish(
        &self,
        operation_id: &str,
        status: OperationStatus,
        completed_steps: u32,
    ) -> Result<()> {
        self.finish_with_locator(operation_id, status, completed_steps, None)
    }
    /// Atomically stores the terminal state and an optional confirmed move locator.
    fn finish_with_locator(
        &self,
        operation_id: &str,
        status: OperationStatus,
        completed_steps: u32,
        locator: Option<&MailResultLocator>,
    ) -> Result<()>;
    /// Removes succeeded and safely failed rows older than 90 days.
    ///
    /// Pending, unknown, and partial rows are retained for manual reconciliation.
    fn prune(&self) -> Result<usize>;
    /// Removes all operation metadata for one remotely wiped account.
    fn purge_account(&self, account_id: &str) -> Result<usize>;
}

/// Computes a deterministic HMAC without storing the canonical payload.
pub fn payload_fingerprint(key: &[u8], payload: &[u8]) -> Result<String> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key)
        .map_err(|_| AppError::new(ErrorCode::StorageError, "invalid journal HMAC key"))?;
    mac.update(payload);
    Ok(mac.finalize().into_bytes().iter().map(|byte| format!("{byte:02x}")).collect())
}

fn storage_error() -> AppError {
    AppError::new(ErrorCode::StorageError, "operation journal is unavailable")
}

#[cfg(test)]
mod tests;
