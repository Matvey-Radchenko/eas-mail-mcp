use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};

use crate::platform;
use crate::references::Clock;
use crate::sanitize::safe_filename;
use crate::{AppError, ErrorCode, Result};

mod usage;

const RETENTION_HOURS: i64 = 24;

pub(super) struct AttachmentCache {
    root: PathBuf,
    clock: Arc<dyn Clock>,
}

impl AttachmentCache {
    pub(super) fn new(root: PathBuf, clock: Arc<dyn Clock>) -> Result<Self> {
        let cache = Self::open(root, clock)?;
        let _guard = cache.lock()?;
        cache.prune()?;
        Ok(cache)
    }

    pub(super) fn open(root: PathBuf, clock: Arc<dyn Clock>) -> Result<Self> {
        private_directory(&root)?;
        Ok(Self { root, clock })
    }

    pub(super) fn store(
        &self,
        account_id: &str,
        token: &str,
        display_name: &str,
        bytes: &[u8],
    ) -> Result<(PathBuf, DateTime<Utc>)> {
        let _guard = self.lock()?;
        self.prune()?;
        let directory = self.account_directory(account_id);
        private_directory(&directory)?;
        let path =
            directory.join(format!("{}_{}", safe_filename(token), safe_filename(display_name)));
        private_file(&path, bytes)?;
        Ok((path, self.clock.now() + Duration::hours(RETENTION_HOURS)))
    }

    pub(super) fn purge_account(&self, account_id: &str) -> Result<()> {
        let _guard = self.lock()?;
        self.remove_account(account_id)
    }

    fn remove_account(&self, account_id: &str) -> Result<()> {
        let path = self.account_directory(account_id);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => remove_entry(&path, &metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(storage_error(error)),
        }
    }

    fn lock(&self) -> Result<fs::File> {
        // Keep the lock outside directories removed by clear and account purges. Opening a
        // distinct handle for each operation also serializes threads in this process.
        let file = platform::open_private_append(&self.root.with_extension("lock"))
            .map_err(storage_error)?;
        file.lock().map_err(storage_error)?;
        private_directory(&self.root)?;
        Ok(file)
    }

    fn prune(&self) -> Result<()> {
        let entries = fs::read_dir(&self.root).map_err(storage_error)?;
        for entry in entries {
            let path = entry.map_err(storage_error)?.path();
            let Some(metadata) = existing_metadata(&path)? else { continue };
            if platform::is_link_or_reparse(&metadata) || metadata.is_file() {
                remove_entry(&path, &metadata)?;
            } else if metadata.is_dir() {
                self.prune_directory(&path)?;
            }
        }
        Ok(())
    }

    fn prune_directory(&self, directory: &Path) -> Result<()> {
        for entry in fs::read_dir(directory).map_err(storage_error)? {
            let path = entry.map_err(storage_error)?.path();
            let Some(metadata) = existing_metadata(&path)? else { continue };
            let expired = self.expired(&metadata);
            if platform::is_link_or_reparse(&metadata) || !metadata.is_file() || expired {
                remove_entry(&path, &metadata)?;
            }
        }
        Ok(())
    }

    fn account_directory(&self, account_id: &str) -> PathBuf {
        self.root.join(safe_filename(account_id))
    }

    fn expired(&self, metadata: &fs::Metadata) -> bool {
        metadata.modified().map(DateTime::<Utc>::from).map_or(true, |modified| {
            modified + Duration::hours(RETENTION_HOURS) <= self.clock.now()
        })
    }
}

fn remove_entry(path: &Path, metadata: &fs::Metadata) -> Result<()> {
    let result = if platform::is_directory_reparse_point(metadata) {
        // Windows directory links need RemoveDirectory; never recurse into their target.
        fs::remove_dir(path)
    } else if platform::is_link_or_reparse(metadata) || metadata.is_file() {
        fs::remove_file(path)
    } else {
        fs::remove_dir_all(path)
    };
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_error(error)),
    }
}

fn existing_metadata(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(storage_error(error)),
    }
}

fn private_directory(path: &Path) -> Result<()> {
    platform::ensure_private_directory(path).map_err(storage_error)
}

fn private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = platform::open_private_new(path).map_err(storage_error)?;
    let result = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result.map_err(storage_error)
}

fn storage_error(_: std::io::Error) -> AppError {
    AppError::new(ErrorCode::StorageError, "managed attachment cache is unavailable")
}

#[cfg(test)]
mod tests;
