use std::collections::BTreeMap;
use std::sync::Mutex;

use eas_mail_mcp::{
    AppError, ErrorCode, JournalBegin, JournalRecord, OperationJournal, OperationStatus, Result,
};

/// Deterministic operation journal for unit and subprocess harnesses.
#[derive(Debug, Default)]
pub struct MemoryJournal {
    records: Mutex<BTreeMap<String, JournalRecord>>,
}

impl OperationJournal for MemoryJournal {
    fn lookup(&self, operation_id: &str) -> Result<Option<JournalRecord>> {
        self.records
            .lock()
            .map(|records| records.get(operation_id).cloned())
            .map_err(|_| storage_error())
    }

    fn begin(&self, record: &JournalRecord) -> Result<JournalBegin> {
        let mut records = self.records.lock().map_err(|_| storage_error())?;
        if let Some(existing) = records.get(&record.operation_id) {
            if existing.account_id != record.account_id
                || existing.kind != record.kind
                || existing.payload_hmac != record.payload_hmac
            {
                return Err(AppError::new(
                    ErrorCode::IdempotencyConflict,
                    "idempotency key was already used for different input",
                ));
            }
            return Ok(JournalBegin { record: existing.clone(), inserted: false });
        }
        records.insert(record.operation_id.clone(), record.clone());
        Ok(JournalBegin { record: record.clone(), inserted: true })
    }

    fn finish(&self, operation_id: &str, status: OperationStatus) -> Result<()> {
        let mut records = self.records.lock().map_err(|_| storage_error())?;
        let record = records.get_mut(operation_id).ok_or_else(storage_error)?;
        record.status = status;
        Ok(())
    }

    fn prune(&self) -> Result<usize> {
        Ok(0)
    }

    fn purge_account(&self, account_id: &str) -> Result<usize> {
        let mut records = self.records.lock().map_err(|_| storage_error())?;
        let before = records.len();
        records.retain(|_, record| record.account_id != account_id);
        Ok(before.saturating_sub(records.len()))
    }
}

fn storage_error() -> AppError {
    AppError::new(ErrorCode::StorageError, "memory journal is unavailable")
}
