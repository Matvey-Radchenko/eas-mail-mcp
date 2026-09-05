use std::sync::Arc;

use clap::Subcommand;

use super::{CliExit, terminal::Terminal};
use crate::attachment_cache::AttachmentCache;
use crate::{AppError, ErrorCode, Paths, Result, SystemClock};

#[derive(Debug, Subcommand)]
pub(super) enum CacheCommand {
    /// Show aggregate usage without pruning expired downloads.
    Status,
    /// Remove downloaded attachments; configuration, secrets, and operation history remain.
    Clear {
        /// Clear only this local account's downloaded attachments.
        #[arg(long, value_name = "ID")]
        account: Option<String>,
        /// Confirm deletion without an interactive prompt.
        #[arg(long)]
        yes: bool,
    },
}

pub(super) fn run(
    paths: &Paths,
    command: CacheCommand,
    terminal: &mut dyn Terminal,
) -> Result<CliExit> {
    let cache = AttachmentCache::open(paths.attachments.clone(), Arc::new(SystemClock))?;
    let value = match command {
        CacheCommand::Status => serde_json::to_value(cache.status()?),
        CacheCommand::Clear { account, yes } => {
            if account.as_deref().is_some_and(|id| !crate::config::valid_account_id(id)) {
                return Err(AppError::new(
                    ErrorCode::ValidationFailed,
                    "invalid account identifier",
                ));
            }
            if !yes && !terminal.confirm("Delete the selected downloaded attachments?", false)? {
                return Ok(CliExit::Declined);
            }
            serde_json::to_value(cache.clear(account.as_deref())?)
        }
    }
    .map_err(|_| AppError::new(ErrorCode::ProtocolError, "cannot serialize cache output"))?;
    super::emit(&value)?;
    Ok(CliExit::Success)
}

#[cfg(test)]
mod tests;
