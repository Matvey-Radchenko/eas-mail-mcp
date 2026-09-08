use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use eas_mail_protocol::{CalendarFields, ChangeData, ChangeKind, Patch, SyncPage};
use serde::{Deserialize, Serialize};

use crate::{AppError, ErrorCode, Result, platform};

#[cfg(test)]
mod tests;

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Snapshot {
    pub(crate) key: String,
    pub(crate) events: BTreeMap<String, CalendarFields>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Collection {
    pub(crate) filter: u8,
    pub(crate) current: Option<Snapshot>,
    pub(crate) rebuilding: Option<Snapshot>,
    pub(crate) more_available: bool,
}

impl Collection {
    pub(crate) fn restart(&mut self, filter: u8) {
        self.filter = filter;
        self.rebuilding = Some(Snapshot { key: "0".into(), ..Snapshot::default() });
        self.more_available = true;
    }

    pub(crate) fn key(&self) -> &str {
        self.rebuilding.as_ref().or(self.current.as_ref()).map_or("0", |value| value.key.as_str())
    }

    pub(crate) fn apply(&mut self, page: SyncPage) -> Result<usize> {
        if page.sync_key.is_empty() || page.sync_key == "0" {
            return Err(AppError::new(
                ErrorCode::ProtocolError,
                "Calendar Sync returned an invalid key",
            ));
        }
        let initialized = self.key() == "0";
        let count = page.changes.len();
        let target =
            self.rebuilding.as_mut().or(self.current.as_mut()).ok_or_else(storage_error)?;
        for change in page.changes {
            match (change.kind, change.data) {
                (ChangeKind::Add | ChangeKind::Change, ChangeData::Calendar(fields)) => {
                    if change.server_id.is_empty() {
                        return Err(AppError::new(
                            ErrorCode::ProtocolError,
                            "Calendar change has no identifier",
                        ));
                    }
                    if change.kind == ChangeKind::Change
                        && !target.events.contains_key(&change.server_id)
                    {
                        return Err(AppError::new(
                            ErrorCode::SyncStale,
                            "Calendar change has no matching baseline item",
                        ));
                    }
                    merge_fields(target.events.entry(change.server_id).or_default(), fields);
                }
                (ChangeKind::Delete | ChangeKind::SoftDelete, _) => {
                    target.events.remove(&change.server_id);
                }
                _ => {
                    return Err(AppError::new(
                        ErrorCode::ProtocolError,
                        "Unexpected Calendar change type",
                    ));
                }
            }
        }
        if target.events.len() > 20_000 {
            return Err(AppError::new(
                ErrorCode::ResultTooLarge,
                "Calendar cache exceeds 20000 master events",
            ));
        }
        target.key = page.sync_key;
        self.more_available = initialized || page.more_available;
        if !self.more_available && self.rebuilding.is_some() {
            self.current = self.rebuilding.take();
        }
        Ok(count)
    }
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct AccountCache {
    pub(crate) version: u8,
    pub(crate) identity: String,
    pub(crate) collections: BTreeMap<String, Collection>,
}

pub(crate) struct CalendarCache {
    root: PathBuf,
}

impl CalendarCache {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn path(&self, account: &str) -> Result<PathBuf> {
        if !crate::config::valid_account_id(account) {
            return Err(storage_error());
        }
        Ok(self.root.join(format!("{account}.json")))
    }

    pub(crate) fn load(&self, account: &str, identity: &str) -> Result<AccountCache> {
        let path = self.path(account)?;
        platform::reject_existing_link(&path).map_err(|_| storage_error())?;
        match fs::metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AccountCache {
                    version: 1,
                    identity: identity.into(),
                    ..AccountCache::default()
                });
            }
            Ok(metadata) if metadata.len() <= 64 * 1024 * 1024 => (),
            _ => return Err(storage_error()),
        }
        platform::protect_file(&path).map_err(|_| storage_error())?;
        let data = fs::read(path).map_err(|_| storage_error())?;
        let cache: AccountCache = serde_json::from_slice(&data).map_err(|_| storage_error())?;
        if cache.version != 1 {
            return Err(storage_error());
        }
        if cache.identity != identity {
            return Err(AppError::new(
                ErrorCode::SyncStale,
                "Calendar cache belongs to a different account configuration",
            ));
        }
        Ok(cache)
    }

    pub(crate) fn save(&self, account: &str, cache: &AccountCache) -> Result<()> {
        let bytes = serde_json::to_vec(cache).map_err(|_| storage_error())?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(storage_error());
        }
        platform::atomic_write(&self.path(account)?, &bytes).map_err(|_| storage_error())
    }

    pub(crate) fn purge(&self, account: &str) -> Result<()> {
        let path = self.path(account)?;
        platform::reject_existing_link(&path).map_err(|_| storage_error())?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(storage_error()),
        }
    }
}

fn storage_error() -> AppError {
    AppError::new(
        ErrorCode::StorageError,
        "Private Calendar sync cache is unavailable or incompatible",
    )
}

fn merge_fields(target: &mut CalendarFields, patch: CalendarFields) {
    if let Some(properties) = patch.properties {
        let old = target.properties.get_or_insert_default();
        if matches!(patch.recurrence, Patch::Value(_)) {
            old.recurrence = properties.recurrence;
        }
        if matches!(patch.exceptions, Patch::Value(_)) {
            old.exceptions = properties.exceptions;
        }
        if properties.sensitivity.is_some() {
            old.sensitivity = properties.sensitivity;
        }
        if properties.categories.is_some() {
            old.categories = properties.categories;
        }
        if properties.appointment_reply_time.is_some() {
            old.appointment_reply_time = properties.appointment_reply_time;
        }
        if properties.online_meeting_conf_link.is_some() {
            old.online_meeting_conf_link = properties.online_meeting_conf_link;
        }
        if properties.online_meeting_external_link.is_some() {
            old.online_meeting_external_link = properties.online_meeting_external_link;
        }
        old.unsupported |= properties.unsupported;
    }
    apply(&mut target.subject, patch.subject);
    apply(&mut target.body, patch.body);
    apply(&mut target.body_truncated, patch.body_truncated);
    apply(&mut target.starts_at, patch.starts_at);
    apply(&mut target.ends_at, patch.ends_at);
    apply(&mut target.all_day, patch.all_day);
    apply(&mut target.location, patch.location);
    apply(&mut target.organizer, patch.organizer);
    apply(&mut target.organizer_email, patch.organizer_email);
    apply(&mut target.attendees, patch.attendees);
    apply(&mut target.reminder_minutes, patch.reminder_minutes);
    apply(&mut target.recurrence, patch.recurrence);
    apply(&mut target.exceptions, patch.exceptions);
    apply(&mut target.meeting_status, patch.meeting_status);
    apply(&mut target.uid, patch.uid);
    apply(&mut target.dt_stamp, patch.dt_stamp);
    apply(&mut target.time_zone, patch.time_zone);
    apply(&mut target.busy_status, patch.busy_status);
    apply(&mut target.response_requested, patch.response_requested);
    apply(&mut target.response_type, patch.response_type);
}

fn apply<T>(target: &mut Patch<T>, value: Patch<T>) {
    if let Patch::Value(_) = value {
        *target = value;
    }
}
