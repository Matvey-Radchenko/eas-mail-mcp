use chrono::{DateTime, Utc};

use crate::model::{OperationGetInput, OperationMetadata, OperationsData, OperationsListInput};
use crate::write_lock::WriteLocks;
use crate::{
    ApiResponse, AppError, ErrorCode, JournalEntry, JournalFilter, OperationJournal, Result,
};

impl super::Runtime {
    /// Inspects a UUID without resending or resolving the external mutation.
    pub fn operation_get(&self, input: OperationGetInput) -> ApiResponse<OperationMetadata> {
        Self::response(
            get(self.journal.as_ref(), &self.write_locks, input).map(|value| (value, Vec::new())),
        )
    }

    /// Lists bounded journal metadata without any Exchange request.
    pub fn operations_list(&self, input: OperationsListInput) -> ApiResponse<OperationsData> {
        Self::response(
            list(self.journal.as_ref(), &self.write_locks, input).map(|value| (value, Vec::new())),
        )
    }
}

pub(crate) fn get(
    journal: &dyn OperationJournal,
    locks: &WriteLocks,
    input: OperationGetInput,
) -> Result<OperationMetadata> {
    let id = uuid::Uuid::parse_str(&input.operation_id)
        .map_err(|_| AppError::new(ErrorCode::ValidationFailed, "operation_id must be a UUID"))?
        .to_string();
    let entry = journal.inspect(&id)?.ok_or_else(|| {
        AppError::new(ErrorCode::NotFound, "operation is not in the retained journal")
    })?;
    recover(journal, locks, Some(&entry.record.account_id))?;
    metadata(
        journal
            .inspect(&id)?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "operation is no longer retained"))?,
    )
}

pub(crate) fn list(
    journal: &dyn OperationJournal,
    locks: &WriteLocks,
    input: OperationsListInput,
) -> Result<OperationsData> {
    let filter = JournalFilter {
        account_id: input.account_id,
        status: input.status,
        limit: input.limit.unwrap_or(20),
    };
    filter.validate()?;
    recover(journal, locks, filter.account_id.as_deref())?;
    let operations = journal.list(&filter)?.into_iter().map(metadata).collect::<Result<_>>()?;
    Ok(OperationsData { operations })
}

fn recover(
    journal: &dyn OperationJournal,
    locks: &WriteLocks,
    account: Option<&str>,
) -> Result<()> {
    for id in journal.pending_accounts()? {
        if account.is_none_or(|account| account == id)
            && let Some(_guard) = locks.try_acquire(&id)?
        {
            journal.recover_account(&id)?;
        }
    }
    Ok(())
}

fn metadata(entry: JournalEntry) -> Result<OperationMetadata> {
    let timestamp = |value| {
        DateTime::<Utc>::from_timestamp(value, 0)
            .ok_or_else(|| AppError::new(ErrorCode::StorageError, "operation timestamp is invalid"))
    };
    Ok(OperationMetadata {
        operation_id: entry.record.operation_id,
        account_id: entry.record.account_id,
        kind: entry.record.kind,
        status: entry.record.status,
        created_at: timestamp(entry.created_at)?,
        updated_at: timestamp(entry.updated_at)?,
        completed_steps: entry.record.completed_steps,
    })
}
