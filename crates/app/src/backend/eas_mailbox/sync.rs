use std::collections::BTreeSet;
use std::sync::Arc;

use eas_mail_protocol::{
    ChangeData, ChangeKind, CollectionKind, EasError, MailFields, Patch, SyncPage,
};
use futures::{StreamExt as _, stream};

use super::super::BackendSync;
use super::session::{CollectionState, EasMailbox, SessionState};
use crate::{AppError, ErrorCode, Result};

const MAX_SYNC_PAGES: usize = 100;
const MAX_CONCURRENT_SYNCS: usize = 8;

struct SyncRequest {
    folder_id: String,
    kind: CollectionKind,
    sync_key: String,
    policy_key: u32,
    filter: u8,
    preview_size: usize,
}

impl EasMailbox {
    pub(super) async fn primary_mail_folder_ids(&self) -> Result<Vec<String>> {
        if self.state.lock().await.folders.is_empty() {
            self.refresh_folders().await?;
        }
        let state = self.state.lock().await;
        let folder_ids = state
            .folders
            .values()
            .filter(|folder| matches!(folder.folder_type, 2 | 5))
            .map(|folder| folder.server_id.clone())
            .collect::<Vec<_>>();
        if folder_ids.is_empty() {
            return Err(AppError::new(
                ErrorCode::ProtocolError,
                "Exchange returned no Inbox or Sent collection",
            )
            .account(&self.account.account_id));
        }
        Ok(folder_ids)
    }

    pub(super) async fn refresh_folders(&self) -> Result<Vec<eas_mail_protocol::Folder>> {
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        let mut page = self.client.folder_sync(state.policy_key, &state.folder_sync_key).await;
        if matches!(page, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            page = self.client.folder_sync(state.policy_key, &state.folder_sync_key).await;
        }
        if matches!(page, Err(EasError::InvalidFolderSyncKey)) && state.folder_sync_key != "0" {
            state.folder_sync_key = "0".into();
            state.folders.clear();
            page = self.client.folder_sync(state.policy_key, "0").await;
        }
        let page = page.map_err(self.scoped_error())?;
        state.folder_sync_key = page.sync_key;
        for id in page.deleted_ids {
            state.folders.remove(&id);
            state.collections.remove(&id);
        }
        for folder in page.folders {
            state.folders.insert(folder.server_id.clone(), folder);
        }
        Ok(state.folders.values().cloned().collect())
    }

    pub(super) async fn sync_mail_selected(
        &self,
        refresh_folders: bool,
        folder_ids: Option<&[String]>,
    ) -> Result<BackendSync> {
        let folders_missing = self.state.lock().await.folders.is_empty();
        if refresh_folders || folders_missing {
            self.refresh_folders().await?;
        }
        let mut state = self.state.lock().await;
        let requested =
            folder_ids.map(|values| values.iter().map(String::as_str).collect::<BTreeSet<_>>());
        let selected = state
            .folders
            .values()
            .filter_map(|folder| {
                let kind = folder.kind?;
                let matches_kind = kind == CollectionKind::Mail;
                let requested = requested
                    .as_ref()
                    .is_none_or(|values| values.contains(folder.server_id.as_str()));
                (matches_kind && requested).then(|| (folder.server_id.clone(), kind))
            })
            .collect::<Vec<_>>();
        let collection_count = selected.len();
        let mut pending = selected;
        let mut changes = 0;
        for _ in 0..MAX_SYNC_PAGES {
            if pending.is_empty() {
                return Ok(BackendSync { collections: collection_count, changes });
            }
            let requests = prepare_requests(&mut state, pending)?;
            let responses = self.fetch_sync_batch(requests).await;
            let mut next = Vec::new();
            let mut refresh_policy = false;
            for (request, response) in responses {
                match response {
                    Ok(page) => {
                        if page.sync_key.is_empty() || page.sync_key == "0" {
                            return Err(AppError::new(
                                ErrorCode::ProtocolError,
                                "Exchange returned an empty collection SyncKey",
                            )
                            .account(&self.account.account_id));
                        }
                        let needs_next = request.sync_key == "0" || page.more_available;
                        changes = changes.saturating_add(page.changes.len());
                        let collection = state
                            .collections
                            .get_mut(&request.folder_id)
                            .ok_or_else(state_error)?;
                        apply_page(collection, page)?;
                        collection.sync_complete = !needs_next;
                        if needs_next {
                            next.push((request.folder_id, request.kind));
                        }
                    }
                    Err(EasError::InvalidSyncKey) if request.sync_key != "0" => {
                        state
                            .collections
                            .insert(request.folder_id.clone(), CollectionState::new(request.kind));
                        next.push((request.folder_id, request.kind));
                    }
                    Err(EasError::PolicyRefreshRequired) => {
                        refresh_policy = true;
                        next.push((request.folder_id, request.kind));
                    }
                    Err(error) => return Err(self.scoped_error()(error)),
                }
            }
            if refresh_policy {
                self.refresh_policy(&mut state).await?;
            }
            pending = next;
        }
        Err(AppError::new(
            ErrorCode::ProtocolError,
            "Exchange exceeded the collection pagination limit",
        )
        .account(&self.account.account_id))
    }

    async fn fetch_sync_batch(
        &self,
        requests: Vec<SyncRequest>,
    ) -> Vec<(SyncRequest, eas_mail_protocol::Result<SyncPage>)> {
        stream::iter(requests)
            .map(|request| {
                let client = Arc::clone(&self.client);
                async move {
                    let response = client
                        .sync(
                            request.policy_key,
                            &request.folder_id,
                            &request.sync_key,
                            request.kind,
                            request.filter,
                            request.preview_size,
                        )
                        .await;
                    (request, response)
                }
            })
            .buffered(MAX_CONCURRENT_SYNCS)
            .collect()
            .await
    }
}

fn prepare_requests(
    state: &mut SessionState,
    pending: Vec<(String, CollectionKind)>,
) -> Result<Vec<SyncRequest>> {
    let mut requests = Vec::with_capacity(pending.len());
    for (folder_id, kind) in pending {
        let collection = state
            .collections
            .entry(folder_id.clone())
            .or_insert_with(|| CollectionState::new(kind));
        // A cancelled or failed page must never leave a write-ready collection.
        collection.sync_complete = false;
        let sync_key = collection.sync_key.clone();
        let (filter, preview_size) = effective_sync_options(state, kind)?;
        requests.push(SyncRequest {
            folder_id,
            kind,
            sync_key,
            policy_key: state.policy_key,
            filter,
            preview_size,
        });
    }
    Ok(requests)
}

fn apply_page(collection: &mut CollectionState, page: SyncPage) -> Result<()> {
    collection.sync_key = page.sync_key;
    for change in page.changes {
        match (collection.kind, change.kind, change.data) {
            (
                CollectionKind::Mail,
                ChangeKind::Add | ChangeKind::Change,
                ChangeData::Mail(fields),
            ) => {
                patch_mail(collection.mail.entry(change.server_id).or_default(), fields);
            }
            (CollectionKind::Mail, ChangeKind::Delete | ChangeKind::SoftDelete, _) => {
                collection.mail.remove(&change.server_id);
            }
            _ => return Err(state_error()),
        }
    }
    Ok(())
}

fn patch_mail(target: &mut MailFields, patch: MailFields) {
    apply(&mut target.subject, patch.subject);
    apply(&mut target.sender, patch.sender);
    apply(&mut target.recipients, patch.recipients);
    apply(&mut target.cc, patch.cc);
    apply(&mut target.received_at, patch.received_at);
    apply(&mut target.body, patch.body);
    apply(&mut target.body_truncated, patch.body_truncated);
    apply(&mut target.is_read, patch.is_read);
    apply(&mut target.importance, patch.importance);
    apply(&mut target.attachments, patch.attachments);
    apply(&mut target.message_class, patch.message_class);
    apply(&mut target.meeting_request, patch.meeting_request);
    apply(&mut target.conversation_id, patch.conversation_id);
    apply(&mut target.conversation_index, patch.conversation_index);
    apply(&mut target.flag, patch.flag);
    apply(&mut target.categories, patch.categories);
}

fn apply<T>(target: &mut Patch<T>, patch: Patch<T>) {
    if let Patch::Value(value) = patch {
        *target = Patch::Value(value);
    }
}

fn state_error() -> AppError {
    AppError::new(ErrorCode::ProtocolError, "process-local Exchange state is inconsistent")
}

fn policy(state: &SessionState) -> Result<&eas_mail_protocol::protocol::PolicyDecision> {
    state.policy.as_ref().ok_or_else(state_error)
}

fn effective_sync_options(state: &SessionState, kind: CollectionKind) -> Result<(u8, usize)> {
    let policy = policy(state)?;
    if kind != CollectionKind::Mail {
        return Err(state_error());
    }
    let filter = policy.mail_filter_type;
    Ok((filter, policy.body_limit.min(500)))
}
