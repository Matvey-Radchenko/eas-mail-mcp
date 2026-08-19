use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};

use crate::platform;
use crate::references::Clock;
use crate::sanitize::safe_filename;
use crate::{AppError, ErrorCode, Result};

const RETENTION_HOURS: i64 = 24;

pub(super) struct AttachmentCache {
    root: PathBuf,
    clock: Arc<dyn Clock>,
}

impl AttachmentCache {
    pub(super) fn new(root: PathBuf, clock: Arc<dyn Clock>) -> Result<Self> {
        private_directory(&root)?;
        let cache = Self { root, clock };
        cache.prune()?;
        Ok(cache)
    }

    pub(super) fn store(
        &self,
        account_id: &str,
        token: &str,
        display_name: &str,
        bytes: &[u8],
    ) -> Result<(PathBuf, DateTime<Utc>)> {
        self.prune()?;
        let directory = self.account_directory(account_id);
        private_directory(&directory)?;
        let path = directory.join(format!("{token}_{}", safe_filename(display_name)));
        private_file(&path, bytes)?;
        Ok((path, self.clock.now() + Duration::hours(RETENTION_HOURS)))
    }

    pub(super) fn purge_account(&self, account_id: &str) -> Result<()> {
        let path = self.account_directory(account_id);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if platform::is_link_or_reparse(&metadata) || metadata.is_file() => {
                fs::remove_file(path).map_err(storage_error)
            }
            Ok(_) => fs::remove_dir_all(path).map_err(storage_error),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(storage_error(error)),
        }
    }

    fn prune(&self) -> Result<()> {
        let entries = fs::read_dir(&self.root).map_err(storage_error)?;
        for entry in entries {
            let path = entry.map_err(storage_error)?.path();
            let metadata = fs::symlink_metadata(&path).map_err(storage_error)?;
            if platform::is_link_or_reparse(&metadata) || metadata.is_file() {
                fs::remove_file(path).map_err(storage_error)?;
            } else if metadata.is_dir() {
                self.prune_directory(&path)?;
            }
        }
        Ok(())
    }

    fn prune_directory(&self, directory: &Path) -> Result<()> {
        for entry in fs::read_dir(directory).map_err(storage_error)? {
            let path = entry.map_err(storage_error)?.path();
            let metadata = fs::symlink_metadata(&path).map_err(storage_error)?;
            let expired = metadata.modified().map(DateTime::<Utc>::from).map_or(true, |modified| {
                modified + Duration::hours(RETENTION_HOURS) <= self.clock.now()
            });
            if platform::is_link_or_reparse(&metadata) || !metadata.is_file() || expired {
                remove_entry(&path, &metadata)?;
            }
        }
        Ok(())
    }

    fn account_directory(&self, account_id: &str) -> PathBuf {
        self.root.join(safe_filename(account_id))
    }
}

fn remove_entry(path: &Path, metadata: &fs::Metadata) -> Result<()> {
    if platform::is_link_or_reparse(metadata) || metadata.is_file() {
        fs::remove_file(path).map_err(storage_error)
    } else {
        fs::remove_dir_all(path).map_err(storage_error)
    }
}

fn private_directory(path: &Path) -> Result<()> {
    platform::ensure_private_directory(path).map_err(storage_error)
}

fn private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = platform::open_private_new(path).map_err(storage_error)?;
    file.write_all(bytes).map_err(storage_error)?;
    file.sync_all().map_err(storage_error)
}

fn storage_error(_: std::io::Error) -> AppError {
    AppError::new(ErrorCode::StorageError, "managed attachment cache is unavailable")
}

#[cfg(test)]
mod tests;
