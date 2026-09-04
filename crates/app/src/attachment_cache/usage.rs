use super::{AttachmentCache, existing_metadata, fs, platform, remove_entry, storage_error};
use crate::{CacheClearData, CacheStatusData, Result};

impl AttachmentCache {
    pub(crate) fn status(&self) -> Result<CacheStatusData> {
        let _guard = self.lock()?;
        self.usage()
    }

    pub(crate) fn clear(&self, account_id: Option<&str>) -> Result<CacheClearData> {
        let _guard = self.lock()?;
        let before = self.usage()?;
        if let Some(account_id) = account_id {
            self.remove_account(account_id)?;
        } else {
            for entry in fs::read_dir(&self.root).map_err(storage_error)? {
                let path = entry.map_err(storage_error)?.path();
                if let Some(metadata) = existing_metadata(&path)? {
                    remove_entry(&path, &metadata)?;
                }
            }
        }
        let after = self.usage()?;
        Ok(CacheClearData {
            removed_files: before.files.saturating_sub(after.files),
            removed_bytes: before.bytes.saturating_sub(after.bytes),
            remaining_files: after.files,
            remaining_bytes: after.bytes,
        })
    }

    fn usage(&self) -> Result<CacheStatusData> {
        let mut result = CacheStatusData {
            retention_hours: 24,
            cleanup_policy: "lazy: at runtime startup and before attachment downloads".into(),
            ..CacheStatusData::default()
        };
        let mut pending = vec![self.root.clone()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory).map_err(storage_error)? {
                let path = entry.map_err(storage_error)?.path();
                let Some(metadata) = existing_metadata(&path)? else { continue };
                if platform::is_link_or_reparse(&metadata) {
                    continue;
                }
                if metadata.is_dir() {
                    pending.push(path);
                } else if metadata.is_file() {
                    result.files = result.files.saturating_add(1);
                    result.bytes = result.bytes.saturating_add(metadata.len());
                    if self.expired(&metadata) {
                        result.expired_files = result.expired_files.saturating_add(1);
                        result.expired_bytes = result.expired_bytes.saturating_add(metadata.len());
                    }
                }
            }
        }
        Ok(result)
    }
}
