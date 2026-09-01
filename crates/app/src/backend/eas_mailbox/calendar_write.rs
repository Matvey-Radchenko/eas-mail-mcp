use eas_mail_protocol::{
    CalendarApplication, CollectionKind, Command, EasError, MeetingResponseChoice,
};

use super::super::{BackendCalendarMutation, BackendEvent};
use super::calendar_write_model::{
    backend_event, calendar_filter, current_calendar_key, missing_during, require_status,
    required_string, source_ids, validate_mutation,
};
use super::session::{CollectionState, EasMailbox, SessionState};
use crate::{AppError, ErrorCode, Result};

impl EasMailbox {
    pub(super) async fn add_event(
        &self,
        client_id: &str,
        item: &BackendCalendarMutation,
    ) -> Result<BackendEvent> {
        let collection_id = if let Some(target) = &item.target_collection {
            if !self.calendar_folder_ids().await?.iter().any(|(_, id)| id == target) {
                return Err(AppError::new(
                    ErrorCode::NotFound,
                    "target Calendar folder is unavailable",
                ));
            }
            target.clone()
        } else {
            self.default_calendar_id().await?
        };
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        self.require_personal_calendar_writes(&state)?;
        let sync_key = self.initialize_calendar(&mut state, &collection_id).await?;
        let result = self
            .calendar_add_with_recovery(
                &mut state,
                &collection_id,
                &sync_key,
                client_id,
                &item.application,
            )
            .await?;
        let server_id = result.server_id.ok_or_else(|| {
            AppError::new(ErrorCode::ProtocolError, "Calendar Add returned no ServerId")
                .account(&self.account.account_id)
        })?;
        self.apply_mutation_key(&mut state, &collection_id, result.sync_key)?;
        let event = backend_event(self, collection_id, server_id, &item.application);
        drop(state);
        self.mutable_event(&event).await
    }

    pub(super) async fn change_event(
        &self,
        source: &BackendEvent,
        item: &BackendCalendarMutation,
    ) -> Result<BackendEvent> {
        let source = self.mutation_source(source).await?;
        let (collection_id, server_id) = source_ids(&source)?;
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        self.require_personal_calendar_writes(&state)?;
        let sync_key = self.initialize_calendar(&mut state, collection_id).await?;
        let (result, current_server_id) = self
            .calendar_change_with_recovery(
                &mut state,
                collection_id,
                server_id,
                &sync_key,
                &item.application,
            )
            .await?;
        self.apply_mutation_key(&mut state, collection_id, result.sync_key)?;
        let event =
            backend_event(self, collection_id.to_owned(), current_server_id, &item.application);
        self.remember_calendar_binding(&mut state, &event)?;
        Ok(event)
    }

    pub(super) async fn delete_event(&self, source: &BackendEvent) -> Result<()> {
        let source = self.mutation_source(source).await?;
        let (collection_id, server_id) = source_ids(&source)?;
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        self.require_personal_calendar_writes(&state)?;
        let sync_key = self.initialize_calendar(&mut state, collection_id).await?;
        let uid = required_string(&source.fields.uid, "Calendar item has no UID")?;
        let result = self
            .calendar_delete_with_recovery(&mut state, collection_id, server_id, &sync_key, uid)
            .await?;
        self.apply_mutation_key(&mut state, collection_id, result.sync_key)?;
        state.calendar_bindings.remove(uid);
        Ok(())
    }

    pub(super) async fn respond_event(
        &self,
        source: &BackendEvent,
        response: MeetingResponseChoice,
    ) -> Result<Option<String>> {
        {
            let mut state = self.state.lock().await;
            self.ensure_ready(&mut state).await?;
            self.require_calendar_capability(&state, Command::MeetingResponse, "MeetingResponse")?;
        }
        let source = self.mutation_source(source).await?;
        let (collection_id, server_id) = source_ids(&source)?;
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        self.require_calendar_capability(&state, Command::MeetingResponse, "MeetingResponse")?;
        let result = self
            .client
            .meeting_response_instance(
                state.policy_key,
                collection_id,
                server_id,
                response,
                source.occurrence_start,
            )
            .await;
        let result = if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            self.client
                .meeting_response_instance(
                    state.policy_key,
                    collection_id,
                    server_id,
                    response,
                    source.occurrence_start,
                )
                .await
        } else {
            result
        }
        .map_err(self.scoped_error())?;
        require_status(result.status, "MeetingResponse")?;
        Ok(result.calendar_id)
    }

    pub(super) async fn send_calendar_mime(&self, client_id: &str, mime: Vec<u8>) -> Result<()> {
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        self.require_calendar_capability(&state, Command::SendMail, "calendar notifications")?;
        let result = self.client.send(state.policy_key, client_id, mime.clone()).await;
        let result = if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            self.client.send(state.policy_key, client_id, mime).await
        } else {
            result
        }
        .map_err(self.scoped_error())?;
        require_status(result.status, "calendar SendMail")
    }

    async fn default_calendar_id(&self) -> Result<String> {
        let folders = self.calendar_folder_ids().await?;
        folders
            .iter()
            .find(|(folder_type, _)| *folder_type == 8)
            .or_else(|| folders.first())
            .cloned()
            .map(|(_, id)| id)
            .ok_or_else(|| {
                AppError::new(ErrorCode::NotFound, "Exchange returned no default Calendar folder")
                    .account(&self.account.account_id)
            })
    }

    pub(super) async fn calendar_folder_ids(&self) -> Result<Vec<(u16, String)>> {
        if self.state.lock().await.folders.is_empty() {
            self.refresh_folders().await?;
        }
        let state = self.state.lock().await;
        let folders = state
            .folders
            .values()
            .filter(|folder| folder.kind == Some(CollectionKind::Calendar))
            .map(|folder| (folder.folder_type, folder.server_id.clone()))
            .collect::<Vec<_>>();
        if folders.is_empty() {
            Err(AppError::new(ErrorCode::NotFound, "Exchange returned no Calendar folders")
                .account(&self.account.account_id))
        } else {
            Ok(folders)
        }
    }

    pub(super) async fn initialize_calendar(
        &self,
        state: &mut SessionState,
        collection_id: &str,
    ) -> Result<String> {
        let existing = state
            .collections
            .entry(collection_id.to_owned())
            .or_insert_with(|| CollectionState::new(CollectionKind::Calendar))
            .sync_key
            .clone();
        if existing != "0" {
            return Ok(existing);
        }
        let page = self.read_calendar_page(state, collection_id, "0").await?;
        if page.sync_key.is_empty() {
            return Err(AppError::new(
                ErrorCode::ProtocolError,
                "Exchange returned an empty Calendar SyncKey",
            )
            .account(&self.account.account_id));
        }
        self.set_calendar_key(state, collection_id, &page.sync_key)?;
        Ok(page.sync_key)
    }

    pub(super) async fn read_calendar_page(
        &self,
        state: &mut SessionState,
        collection_id: &str,
        sync_key: &str,
    ) -> Result<eas_mail_protocol::SyncPage> {
        let filter = calendar_filter(state)?;
        let result = self
            .client
            .sync(state.policy_key, collection_id, sync_key, CollectionKind::Calendar, filter, 0)
            .await;
        let result = if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(state).await?;
            self.client
                .sync(
                    state.policy_key,
                    collection_id,
                    sync_key,
                    CollectionKind::Calendar,
                    filter,
                    0,
                )
                .await
        } else {
            result
        };
        result.map_err(self.scoped_error())
    }

    async fn calendar_add_with_recovery(
        &self,
        state: &mut SessionState,
        collection_id: &str,
        sync_key: &str,
        client_id: &str,
        item: &CalendarApplication,
    ) -> Result<eas_mail_protocol::MutationResult> {
        let mut result = self
            .client
            .calendar_add(state.policy_key, collection_id, sync_key, client_id, item)
            .await;
        if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(state).await?;
            result = self
                .client
                .calendar_add(state.policy_key, collection_id, sync_key, client_id, item)
                .await;
        }
        if matches!(result, Err(EasError::InvalidSyncKey)) {
            self.reset_calendar(state, collection_id);
            let key = self.initialize_calendar(state, collection_id).await?;
            result = self
                .client
                .calendar_add(state.policy_key, collection_id, &key, client_id, item)
                .await;
        }
        result.map_err(self.scoped_error()).and_then(validate_mutation)
    }

    async fn calendar_change_with_recovery(
        &self,
        state: &mut SessionState,
        collection_id: &str,
        server_id: &str,
        sync_key: &str,
        item: &CalendarApplication,
    ) -> Result<(eas_mail_protocol::MutationResult, String)> {
        let mut result = self
            .client
            .calendar_change(state.policy_key, collection_id, server_id, sync_key, item)
            .await;
        if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(state).await?;
            result = self
                .client
                .calendar_change(state.policy_key, collection_id, server_id, sync_key, item)
                .await;
        }
        let mut current_id = server_id.to_owned();
        if matches!(result, Err(EasError::InvalidSyncKey)) {
            self.reset_calendar(state, collection_id);
            let resolved = self
                .scan_calendar_collection(state, collection_id, &item.uid)
                .await?
                .ok_or_else(|| missing_during("update", &self.account.account_id))?;
            current_id = resolved.server_id.ok_or_else(|| {
                AppError::new(ErrorCode::ProtocolError, "Calendar fallback returned no ServerId")
                    .account(&self.account.account_id)
            })?;
            let current_key = current_calendar_key(state, collection_id)?;
            result = self
                .client
                .calendar_change(state.policy_key, collection_id, &current_id, &current_key, item)
                .await;
        }
        result
            .map_err(self.scoped_error())
            .and_then(validate_mutation)
            .map(|result| (result, current_id))
    }

    async fn calendar_delete_with_recovery(
        &self,
        state: &mut SessionState,
        collection_id: &str,
        server_id: &str,
        sync_key: &str,
        uid: &str,
    ) -> Result<eas_mail_protocol::MutationResult> {
        let mut result =
            self.client.calendar_delete(state.policy_key, collection_id, server_id, sync_key).await;
        if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(state).await?;
            result = self
                .client
                .calendar_delete(state.policy_key, collection_id, server_id, sync_key)
                .await;
        }
        if matches!(result, Err(EasError::InvalidSyncKey)) {
            self.reset_calendar(state, collection_id);
            let resolved = self
                .scan_calendar_collection(state, collection_id, uid)
                .await?
                .ok_or_else(|| missing_during("deletion", &self.account.account_id))?;
            let current_id = resolved.server_id.ok_or_else(|| {
                AppError::new(ErrorCode::ProtocolError, "Calendar fallback returned no ServerId")
                    .account(&self.account.account_id)
            })?;
            let current_key = current_calendar_key(state, collection_id)?;
            result = self
                .client
                .calendar_delete(state.policy_key, collection_id, &current_id, &current_key)
                .await;
        }
        result.map_err(self.scoped_error()).and_then(validate_mutation)
    }

    pub(super) fn reset_calendar(&self, state: &mut SessionState, collection_id: &str) {
        state.calendar_bindings.retain(|_, value| value.collection_id != collection_id);
        state
            .collections
            .insert(collection_id.to_owned(), CollectionState::new(CollectionKind::Calendar));
    }

    fn apply_mutation_key(
        &self,
        state: &mut SessionState,
        collection_id: &str,
        sync_key: Option<String>,
    ) -> Result<()> {
        let sync_key = sync_key.ok_or_else(|| {
            AppError::new(ErrorCode::ProtocolError, "Calendar mutation returned no SyncKey")
                .account(&self.account.account_id)
        })?;
        state.calendar_bindings.retain(|_, value| value.collection_id != collection_id);
        self.set_calendar_key(state, collection_id, &sync_key)
    }

    pub(super) fn set_calendar_key(
        &self,
        state: &mut SessionState,
        collection_id: &str,
        sync_key: &str,
    ) -> Result<()> {
        let collection = state.collections.get_mut(collection_id).ok_or_else(|| {
            AppError::new(ErrorCode::ProtocolError, "Calendar collection state is unavailable")
                .account(&self.account.account_id)
        })?;
        collection.sync_key = sync_key.to_owned();
        Ok(())
    }

    fn require_personal_calendar_writes(&self, state: &SessionState) -> Result<()> {
        if state
            .capabilities
            .as_ref()
            .is_some_and(eas_mail_protocol::ServerCapabilities::supports_personal_calendar_writes)
        {
            Ok(())
        } else {
            Err(AppError::new(
                ErrorCode::FeatureUnavailable,
                "Exchange does not advertise Calendar write commands",
            )
            .account(&self.account.account_id))
        }
    }

    pub(super) fn require_calendar_capability(
        &self,
        state: &SessionState,
        command: Command,
        feature: &'static str,
    ) -> Result<()> {
        if state.capabilities.as_ref().is_some_and(|value| value.supports(command)) {
            Ok(())
        } else {
            Err(AppError::new(
                ErrorCode::FeatureUnavailable,
                format!("Exchange does not advertise {feature}"),
            )
            .account(&self.account.account_id))
        }
    }
}
