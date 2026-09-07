use std::fs::{File, TryLockError};
use std::path::PathBuf;
use std::time::Duration;

use crate::config::valid_account_id;
use crate::{AppError, ErrorCode, Result, platform};

const WAIT_LIMIT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

pub(crate) struct WriteLocks {
    directory: PathBuf,
}

impl WriteLocks {
    pub(crate) fn new(directory: PathBuf) -> Result<Self> {
        platform::ensure_private_directory(&directory).map_err(|_| lock_error())?;
        Ok(Self { directory })
    }

    pub(crate) async fn acquire(&self, account_id: &str) -> Result<WriteGuard> {
        self.acquire_with_timeout(account_id, WAIT_LIMIT).await
    }

    pub(crate) fn try_acquire(&self, account_id: &str) -> Result<Option<WriteGuard>> {
        let file = self.open(account_id)?;
        if try_lock(&file)? { Ok(Some(WriteGuard { _file: file })) } else { Ok(None) }
    }

    async fn acquire_with_timeout(
        &self,
        account_id: &str,
        timeout: Duration,
    ) -> Result<WriteGuard> {
        let file = self.open(account_id)?;
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if try_lock(&file)? {
                return Ok(WriteGuard { _file: file });
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(AppError::new(
                    ErrorCode::StorageError,
                    "another operation still holds the account write lock; wait and retry",
                )
                .retryable());
            }
            tokio::time::sleep(POLL_INTERVAL.min(remaining)).await;
        }
    }

    fn open(&self, account_id: &str) -> Result<File> {
        if !valid_account_id(account_id) {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                "invalid account identifier for write lock",
            ));
        }
        let path = self.directory.join(format!("{account_id}.lock"));
        platform::open_private_append(&path).map_err(|_| lock_error())
    }
}

pub(crate) struct WriteGuard {
    _file: File,
}

fn try_lock(file: &File) -> Result<bool> {
    match file.try_lock() {
        Ok(()) => Ok(true),
        Err(TryLockError::WouldBlock) => Ok(false),
        Err(TryLockError::Error(_)) => Err(lock_error()),
    }
}

fn lock_error() -> AppError {
    AppError::new(ErrorCode::StorageError, "per-account write lock is unavailable")
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod process_tests;
