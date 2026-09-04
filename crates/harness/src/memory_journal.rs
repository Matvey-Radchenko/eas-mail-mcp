use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use eas_mail_mcp::{
    AppError, ErrorCode, JournalBegin, JournalEntry, JournalFilter, JournalRecord,
    MailResultLocator, OperationJournal, OperationStatus, Result,
};

/// Deterministic operation journal for unit and subprocess harnesses.
#[derive(Debug, Default)]
pub struct MemoryJournal {
    records: Mutex<BTreeMap<String, JournalEntry>>,
    fail_finish: AtomicBool,
    fail_checkpoint: AtomicBool,
}

impl MemoryJournal {
    /// Makes final state persistence fail while retaining the pending row.
    pub fn set_finish_failure(&self, fail: bool) {
        self.fail_finish.store(fail, Ordering::Relaxed);
    }

    /// Makes checkpoints fail after a backend operation has already completed.
    pub fn set_checkpoint_failure(&self, fail: bool) {
        self.fail_checkpoint.store(fail, Ordering::Relaxed);
    }
}

impl OperationJournal for MemoryJournal {
    fn lookup(&self, operation_id: &str) -> Result<Option<JournalRecord>> {
        self.inspect(operation_id).map(|entry| entry.map(|entry| entry.record))
    }

    fn inspect(&self, operation_id: &str) -> Result<Option<JournalEntry>> {
        self.records
            .lock()
            .map(|records| records.get(operation_id).cloned())
            .map_err(|_| storage_error())
    }

    fn list(&self, filter: &JournalFilter) -> Result<Vec<JournalEntry>> {
        filter.validate()?;
        let records = self.records.lock().map_err(|_| storage_error())?;
        let mut entries = records
            .values()
            .filter(|entry| {
                filter.account_id.as_ref().is_none_or(|id| *id == entry.record.account_id)
                    && filter.status.is_none_or(|status| status == entry.record.status)
            })
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.record.operation_id.cmp(&b.record.operation_id))
        });
        entries.truncate(usize::from(filter.limit));
        Ok(entries)
    }

    fn pending_accounts(&self) -> Result<Vec<String>> {
        let records = self.records.lock().map_err(|_| storage_error())?;
        Ok(records
            .values()
            .filter(|entry| entry.record.status == OperationStatus::Pending)
            .map(|entry| entry.record.account_id.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect())
    }

    fn recover_account(&self, account_id: &str) -> Result<usize> {
        let mut records = self.records.lock().map_err(|_| storage_error())?;
        let mut changed = 0;
        for entry in records.values_mut().filter(|entry| {
            entry.record.account_id == account_id && entry.record.status == OperationStatus::Pending
        }) {
            entry.record.status = OperationStatus::Unknown;
            entry.updated_at += 1;
            changed += 1;
        }
        Ok(changed)
    }

    fn begin(&self, record: &JournalRecord) -> Result<JournalBegin> {
        let mut records = self.records.lock().map_err(|_| storage_error())?;
        if let Some(existing) = records.get(&record.operation_id) {
            let existing = &existing.record;
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
        let timestamp =
            records.values().map(|entry| entry.updated_at).max().unwrap_or(1_700_000_000) + 1;
        records.insert(
            record.operation_id.clone(),
            JournalEntry {
                record: record.clone(),
                created_at: timestamp,
                updated_at: timestamp,
                result_locator: None,
            },
        );
        Ok(JournalBegin { record: record.clone(), inserted: true })
    }

    fn checkpoint(&self, operation_id: &str, completed_steps: u32) -> Result<()> {
        if self.fail_checkpoint.load(Ordering::Relaxed) {
            return Err(storage_error());
        }
        let mut records = self.records.lock().map_err(|_| storage_error())?;
        let entry = records.get_mut(operation_id).ok_or_else(storage_error)?;
        if entry.record.status != OperationStatus::Pending {
            return Err(storage_error());
        }
        entry.record.completed_steps = completed_steps;
        entry.updated_at += 1;
        Ok(())
    }

    fn finish_with_locator(
        &self,
        operation_id: &str,
        status: OperationStatus,
        completed_steps: u32,
        locator: Option<&MailResultLocator>,
    ) -> Result<()> {
        if self.fail_finish.load(Ordering::Relaxed) {
            return Err(storage_error());
        }
        if status == OperationStatus::Pending {
            return Err(storage_error());
        }
        if let Some(locator) = locator {
            locator.validate()?;
            if status != OperationStatus::Succeeded {
                return Err(AppError::new(
                    ErrorCode::ValidationFailed,
                    "a move result locator requires confirmed success",
                ));
            }
        }
        let mut records = self.records.lock().map_err(|_| storage_error())?;
        let entry = records.get_mut(operation_id).ok_or_else(storage_error)?;
        entry.record.status = status;
        entry.record.completed_steps = completed_steps;
        entry.result_locator = locator.cloned();
        entry.updated_at += 1;
        Ok(())
    }

    fn prune(&self) -> Result<usize> {
        Ok(0)
    }

    fn purge_account(&self, account_id: &str) -> Result<usize> {
        let mut records = self.records.lock().map_err(|_| storage_error())?;
        let before = records.len();
        records.retain(|_, entry| entry.record.account_id != account_id);
        Ok(before.saturating_sub(records.len()))
    }
}

fn storage_error() -> AppError {
    AppError::new(ErrorCode::StorageError, "memory journal is unavailable")
}
