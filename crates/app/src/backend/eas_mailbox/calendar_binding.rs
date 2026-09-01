use eas_mail_protocol::{ChangeData, ChangeKind, Patch};

use super::super::BackendEvent;
use super::calendar_write_model::{current_calendar_key, patch_eq, required_string, source_ids};
use super::session::{CalendarBinding, EasMailbox, SessionState};
use crate::{AppError, ErrorCode, Result};

const MAX_CALENDAR_SYNC_PAGES: usize = 100;

impl EasMailbox {
    pub(super) async fn mutable_event(&self, source: &BackendEvent) -> Result<BackendEvent> {
        let referenced_uid = match &source.fields.uid {
            Patch::Value(value) if !value.is_empty() => Some(value.as_str()),
            Patch::Missing | Patch::Value(_) => None,
        };
        if let Some(uid) = referenced_uid {
            return self.refetch_event_by_uid(uid, source).await;
        }
        let event = self.fetch_event(source, 50_000).await?;
        let uid = required_string(&event.fields.uid, "Calendar item has no UID")?;
        self.refetch_event_by_uid(uid, &event).await
    }

    async fn refetch_event_by_uid(&self, uid: &str, source: &BackendEvent) -> Result<BackendEvent> {
        let mut resolved = self.find_event_by_uid(uid, source.collection_id.as_deref()).await?;
        resolved.occurrence_start = source.occurrence_start;
        let fetched = self.fetch_event(&resolved, 50_000).await?;
        if !patch_eq(&fetched.fields.uid, uid) {
            return Err(AppError::new(
                ErrorCode::SyncStale,
                "Calendar item changed while its reference was being resolved",
            )
            .account(&self.account.account_id));
        }
        Ok(fetched)
    }

    async fn find_event_by_uid(
        &self,
        uid: &str,
        preferred_collection: Option<&str>,
    ) -> Result<BackendEvent> {
        let folders = if let Some(preferred) = preferred_collection {
            vec![(0, preferred.to_owned())]
        } else {
            self.calendar_folder_ids().await?
        };
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        for (_, collection_id) in folders {
            if let Some(event) =
                self.scan_calendar_collection(&mut state, &collection_id, uid).await?
            {
                return Ok(event);
            }
        }
        Err(AppError::new(ErrorCode::NotFound, "Calendar item is outside the mutable sync window")
            .account(&self.account.account_id))
    }

    pub(super) async fn scan_calendar_collection(
        &self,
        state: &mut SessionState,
        collection_id: &str,
        uid: &str,
    ) -> Result<Option<BackendEvent>> {
        self.reset_calendar(state, collection_id);
        let mut sync_key = self.initialize_calendar(state, collection_id).await?;
        for _ in 0..MAX_CALENDAR_SYNC_PAGES {
            let page = self.read_calendar_page(state, collection_id, &sync_key).await?;
            sync_key.clone_from(&page.sync_key);
            self.set_calendar_key(state, collection_id, &sync_key)?;
            for change in page.changes {
                if matches!(change.kind, ChangeKind::Add | ChangeKind::Change)
                    && let ChangeData::Calendar(fields) = change.data
                    && patch_eq(&fields.uid, uid)
                {
                    let server_id = change.server_id;
                    state.calendar_bindings.insert(
                        uid.to_owned(),
                        CalendarBinding {
                            collection_id: collection_id.to_owned(),
                            server_id: server_id.clone(),
                            sync_key: sync_key.clone(),
                        },
                    );
                    return Ok(Some(BackendEvent {
                        occurrence_start: None,
                        account_id: self.account.account_id.clone(),
                        long_id: String::new(),
                        collection_id: Some(collection_id.to_owned()),
                        server_id: Some(server_id),
                        fields,
                    }));
                }
            }
            if !page.more_available {
                return Ok(None);
            }
        }
        Err(AppError::new(
            ErrorCode::ProtocolError,
            "Exchange exceeded Calendar fallback pagination limit",
        )
        .account(&self.account.account_id))
    }

    pub(super) async fn mutation_source(&self, source: &BackendEvent) -> Result<BackendEvent> {
        let uid = match &source.fields.uid {
            Patch::Value(value) if !value.is_empty() => value,
            Patch::Missing | Patch::Value(_) => return self.mutable_event(source).await,
        };
        let reusable = {
            let state = self.state.lock().await;
            state.calendar_bindings.get(uid).is_some_and(|binding| {
                source.collection_id.as_deref() == Some(binding.collection_id.as_str())
                    && source.server_id.as_deref() == Some(binding.server_id.as_str())
                    && state
                        .collections
                        .get(&binding.collection_id)
                        .is_some_and(|collection| collection.sync_key == binding.sync_key)
            })
        };
        if reusable { Ok(source.clone()) } else { self.mutable_event(source).await }
    }

    pub(super) fn remember_calendar_binding(
        &self,
        state: &mut SessionState,
        event: &BackendEvent,
    ) -> Result<()> {
        let uid = required_string(&event.fields.uid, "Calendar item has no UID")?;
        let (collection_id, server_id) = source_ids(event)?;
        let sync_key = current_calendar_key(state, collection_id)?;
        state.calendar_bindings.insert(
            uid.to_owned(),
            CalendarBinding {
                collection_id: collection_id.to_owned(),
                server_id: server_id.to_owned(),
                sync_key,
            },
        );
        Ok(())
    }
}
