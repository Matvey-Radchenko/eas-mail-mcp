use super::Runtime;
use crate::model::{OperationResult, OperationState};
use crate::{AppError, ErrorCode, JournalRecord, OperationStatus, Result};

impl Runtime {
    /// A failed local checkpoint cannot make an already attempted mutation safe to repeat.
    pub(super) fn journal_after_mutation<T>(
        result: Result<T>,
        account_id: &str,
        operation_id: &str,
    ) -> Result<T> {
        result.map_err(|_| {
            AppError::new(
                ErrorCode::OutcomeUnknown,
                "Exchange may have applied the operation, but local result handling failed",
            )
            .account(account_id)
            .operation(operation_id)
            .remediation("Inspect the operation and Exchange state; do not retry with a new UUID")
        })
    }

    pub(super) fn checkpoint_mutation(&self, record: &JournalRecord, steps: u32) -> Result<()> {
        Self::journal_after_mutation(
            self.journal.checkpoint(&record.operation_id, steps),
            &record.account_id,
            &record.operation_id,
        )
    }

    pub(super) fn finish_write(
        &self,
        account_id: &str,
        operation_id: &str,
        result: Result<()>,
    ) -> Result<OperationResult> {
        match result {
            Ok(()) => {
                Self::journal_after_mutation(
                    self.journal.finish(operation_id, OperationStatus::Succeeded, 1),
                    account_id,
                    operation_id,
                )?;
                Ok(OperationResult {
                    operation_id: operation_id.into(),
                    status: OperationState::Succeeded,
                    message: "Exchange confirmed the operation".into(),
                })
            }
            Err(error) if error.envelope.code == ErrorCode::OutcomeUnknown => {
                Self::journal_after_mutation(
                    self.journal.finish(operation_id, OperationStatus::Unknown, 0),
                    account_id,
                    operation_id,
                )?;
                Err(error.operation(operation_id))
            }
            Err(error) if error.envelope.code == ErrorCode::RemoteWipe => {
                Self::journal_after_mutation(
                    self.purge_account(account_id),
                    account_id,
                    operation_id,
                )?;
                Err(error.operation(operation_id))
            }
            Err(error) => {
                Self::journal_after_mutation(
                    self.journal.finish(operation_id, OperationStatus::Failed, 0),
                    account_id,
                    operation_id,
                )?;
                Err(error.operation(operation_id))
            }
        }
    }
}
