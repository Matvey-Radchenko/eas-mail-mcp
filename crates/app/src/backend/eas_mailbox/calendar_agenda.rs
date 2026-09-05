use std::collections::BTreeMap;

use eas_mail_protocol::{CalendarFields, ChangeData, ChangeKind, Patch};

use super::session::EasMailbox;
use crate::backend::{BackendCalendarSearch, BackendEvent};
use crate::{AppError, ErrorCode, Result};

const MAX_CALENDAR_PAGES: usize = 100;
const MAX_CALENDAR_ITEMS: usize = 10_000;

impl EasMailbox {
    pub(super) async fn scan_calendar_events(&self) -> Result<BackendCalendarSearch> {
        let folders = self.calendar_folder_ids().await?;
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        let mut events = BTreeMap::new();
        for (_, collection_id) in folders {
            self.reset_calendar(&mut state, &collection_id);
            let initial = self.read_calendar_page(&mut state, &collection_id, "0").await?;
            let mut sync_key = require_sync_key(initial.sync_key)?;
            self.set_calendar_key(&mut state, &collection_id, &sync_key)?;
            apply_page(&mut events, &collection_id, initial.changes)?;
            let mut complete = false;
            for _ in 0..MAX_CALENDAR_PAGES {
                let page = self.read_calendar_page(&mut state, &collection_id, &sync_key).await?;
                sync_key = require_sync_key(page.sync_key)?;
                self.set_calendar_key(&mut state, &collection_id, &sync_key)?;
                let more_available = page.more_available;
                apply_page(&mut events, &collection_id, page.changes)?;
                if events.len() > MAX_CALENDAR_ITEMS {
                    return Err(result_too_large(&self.account.account_id));
                }
                if !more_available {
                    complete = true;
                    break;
                }
            }
            if !complete {
                return Err(result_too_large(&self.account.account_id));
            }
        }
        let total = events.len();
        let events = events
            .into_iter()
            .map(|((collection_id, server_id), fields)| BackendEvent {
                occurrence_start: None,
                account_id: self.account.account_id.clone(),
                long_id: String::new(),
                collection_id: Some(collection_id),
                server_id: Some(server_id),
                fields,
            })
            .collect();
        Ok(BackendCalendarSearch { events, total })
    }
}

fn apply_page(
    events: &mut BTreeMap<(String, String), CalendarFields>,
    collection_id: &str,
    changes: Vec<eas_mail_protocol::SyncChange>,
) -> Result<()> {
    for change in changes {
        let key = (collection_id.to_owned(), change.server_id);
        match (change.kind, change.data) {
            (ChangeKind::Add | ChangeKind::Change, ChangeData::Calendar(fields)) => {
                patch_calendar(events.entry(key).or_default(), fields);
            }
            (ChangeKind::Delete | ChangeKind::SoftDelete, _) => {
                events.remove(&key);
            }
            _ => {
                return Err(AppError::new(
                    ErrorCode::ProtocolError,
                    "Calendar metadata Sync returned an unexpected change",
                ));
            }
        }
    }
    Ok(())
}

fn patch_calendar(target: &mut CalendarFields, patch: CalendarFields) {
    apply(&mut target.subject, patch.subject);
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

fn apply<T>(target: &mut Patch<T>, patch: Patch<T>) {
    if let Patch::Value(value) = patch {
        *target = Patch::Value(value);
    }
}

fn require_sync_key(value: String) -> Result<String> {
    if value.is_empty() {
        Err(AppError::new(ErrorCode::ProtocolError, "Exchange returned an empty Calendar SyncKey"))
    } else {
        Ok(value)
    }
}

fn result_too_large(account_id: &str) -> AppError {
    AppError::new(ErrorCode::ResultTooLarge, "Calendar metadata exceeds the bounded agenda scan")
        .account(account_id)
        .remediation("Shorten mailbox retention or use a text query")
}
