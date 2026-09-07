use clap::{Args, Subcommand};

use crate::model::{OperationGetInput, OperationsListInput};
use crate::runtime::operation_reads;
use crate::write_lock::WriteLocks;
use crate::{ApiResponse, AppError, ErrorCode, Paths, Result, SqliteJournal};

#[derive(Debug, Subcommand)]
pub(super) enum OperationCommand {
    /// Inspect one retained operation without sending another Exchange request.
    Get { operation_id: String },
    /// List newest durable states; no message content is returned.
    List(ListArgs),
}

#[cfg(test)]
mod tests;

#[derive(Debug, Args)]
pub(super) struct ListArgs {
    /// Filter by local account identifier.
    #[arg(long)]
    account: Option<String>,
    /// Filter by pending, succeeded, failed, partial, or unknown.
    #[arg(long, value_parser = ["pending", "succeeded", "failed", "partial", "unknown"])]
    status: Option<String>,
    /// Number of rows, default 20 and maximum 100.
    #[arg(long, default_value_t = 20)]
    limit: u16,
}

pub(super) fn run(paths: &Paths, command: OperationCommand) -> Result<super::CliExit> {
    paths.ensure()?;
    let journal = SqliteJournal::open(&paths.journal)?;
    let root = paths.attachments.parent().ok_or_else(|| {
        AppError::new(ErrorCode::StorageError, "write lock directory is unavailable")
    })?;
    let locks = WriteLocks::new(root.join("write-locks"))?;
    let output = match command {
        OperationCommand::Get { operation_id } => {
            let data = operation_reads::get(&journal, &locks, OperationGetInput { operation_id })?;
            serde_json::to_value(ApiResponse::success(data, Vec::new()))
        }
        OperationCommand::List(args) => {
            let status = args
                .status
                .map(|value| serde_json::from_value(serde_json::Value::String(value)))
                .transpose()
                .map_err(|_| {
                    AppError::new(ErrorCode::ValidationFailed, "invalid operation status")
                })?;
            let data = operation_reads::list(
                &journal,
                &locks,
                OperationsListInput { account_id: args.account, status, limit: Some(args.limit) },
            )?;
            serde_json::to_value(ApiResponse::success(data, Vec::new()))
        }
    }
    .map_err(|_| AppError::new(ErrorCode::ProtocolError, "cannot serialize operation metadata"))?;
    super::emit(&output)?;
    Ok(super::CliExit::Success)
}
