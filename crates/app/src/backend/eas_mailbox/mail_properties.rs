use super::session::{CollectionState, EasMailbox, SessionState};
use crate::backend::{AccountBackend, MailSource};
use crate::{AppError, ErrorCode, Result};
use eas_mail_protocol::protocol::{MailPatch, build_mail_change};
use eas_mail_protocol::wbxml::{Element, decode};
use eas_mail_protocol::{CollectionKind, Command, EasError, Patch};

impl EasMailbox {
    pub(super) async fn check_property_ready(&self, source: &MailSource) -> Result<()> {
        let state = self.state.lock().await;
        property_collection(&state, source).map(|_| ())
    }

    pub(super) async fn change_mail_property(
        &self,
        source: &MailSource,
        patch: &MailPatch,
    ) -> Result<()> {
        let mail = self.resolve_mail_source(source).await?;
        let MailSource::Item { folder_id, server_id } = &mail.source else {
            return Err(AppError::new(
                ErrorCode::FeatureUnavailable,
                "mail item locator is unavailable",
            ));
        };
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        self.require_capability(&state, Command::Sync)?;
        let collection = property_collection(&state, &mail.source)?;
        let flag = prepared_flag(folder_id, server_id, &collection.sync_key, patch)?;
        // Remove before awaiting: cancellation or an uncertain response invalidates this key.
        let mut collection = state.collections.remove(folder_id).ok_or_else(sync_required)?;
        let result = self
            .client
            .mail_change(state.policy_key, folder_id, server_id, &collection.sync_key, patch)
            .await;
        let result = if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            self.client
                .mail_change(state.policy_key, folder_id, server_id, &collection.sync_key, patch)
                .await
        } else {
            result
        }
        .map_err(self.scoped_error())?;
        if matches!(result.status, 3 | 12) {
            return Err(AppError::new(
                ErrorCode::SyncStale,
                format!("Exchange rejected the mail change with status {}", result.status),
            ));
        }
        collection.sync_key =
            result.sync_key.filter(|key| !key.is_empty() && key != "0").ok_or_else(|| {
                AppError::new(ErrorCode::OutcomeUnknown, "mail change returned no SyncKey")
            })?;
        if result.status != 1 {
            if result.status == 8 {
                collection.mail.remove(server_id);
            }
            // A definite item rejection still advances the confirmed collection key.
            state.collections.insert(folder_id.clone(), collection);
            let code =
                if result.status == 8 { ErrorCode::SyncStale } else { ErrorCode::ProtocolError };
            return Err(AppError::new(
                code,
                format!("Exchange rejected the mail change with status {}", result.status),
            ));
        }
        update_cached_property(&mut collection, server_id, patch, flag);
        state.collections.insert(folder_id.clone(), collection);
        Ok(())
    }

    pub(super) async fn move_message(
        &self,
        source: &MailSource,
        destination: &str,
    ) -> Result<MailSource> {
        let mail = self.resolve_mail_source(source).await?;
        let MailSource::Item { folder_id, server_id } = &mail.source else {
            return Err(AppError::new(
                ErrorCode::FeatureUnavailable,
                "mail item locator is unavailable",
            ));
        };
        let folders = self.refresh_folders().await?;
        if !folders.iter().any(|folder| {
            folder.server_id == destination && folder.kind == Some(CollectionKind::Mail)
        }) {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                "destination must be an existing mail folder in this account",
            ));
        }
        if folder_id == destination {
            return Ok(mail.source);
        }
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        self.require_capability(&state, Command::MoveItems)?;
        let result =
            self.client.move_mail(state.policy_key, folder_id, server_id, destination).await;
        let result = if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            self.client.move_mail(state.policy_key, folder_id, server_id, destination).await
        } else {
            result
        }
        .map_err(self.scoped_error())?;
        if result.status != 3 {
            return Err(AppError::new(
                ErrorCode::ProtocolError,
                format!("Exchange rejected MoveItems with status {}", result.status),
            ));
        }
        let destination_server_id = result.server_id.ok_or_else(|| {
            AppError::new(
                ErrorCode::OutcomeUnknown,
                "MoveItems did not confirm the new message identifier",
            )
        })?;
        // MoveItems queues Delete/Add for the next Sync; it does not reset the keys.
        if let Some(collection) = state.collections.get_mut(folder_id) {
            collection.mail.remove(server_id);
        }
        if let Some(collection) = state.collections.get_mut(destination) {
            collection.mail.remove(&destination_server_id);
        }
        Ok(MailSource::Item { folder_id: destination.into(), server_id: destination_server_id })
    }

    pub(super) async fn change_flag(&self, source: &MailSource, status: u8) -> Result<()> {
        let mail = self.resolve_mail_source(source).await?;
        let previous = match mail.fields.flag {
            Patch::Value(value) => Some(value),
            Patch::Missing if status == 0 => None,
            Patch::Missing => {
                return Err(AppError::new(
                    ErrorCode::FeatureUnavailable,
                    "Exchange omitted flag metadata; existing parameters cannot be preserved",
                ));
            }
        };
        self.change_mail_property(
            &mail.source,
            &MailPatch::Flag { status, previous, updated_at: chrono::Utc::now() },
        )
        .await
    }
}

fn property_collection<'a>(
    state: &'a SessionState,
    source: &MailSource,
) -> Result<&'a CollectionState> {
    let MailSource::Item { folder_id, server_id } = source else {
        return Err(AppError::new(
            ErrorCode::FeatureUnavailable,
            "mail item locator is unavailable",
        ));
    };
    let collection = state.collections.get(folder_id).ok_or_else(sync_required)?;
    if !collection.sync_complete || collection.sync_key.is_empty() || collection.sync_key == "0" {
        return Err(sync_required());
    }
    if !collection.mail.contains_key(server_id) {
        return Err(AppError::new(
            ErrorCode::SyncStale,
            "the message is absent from the synchronized folder; list the folder again",
        ));
    }
    Ok(collection)
}

fn sync_required() -> AppError {
    AppError::new(
        ErrorCode::FeatureUnavailable,
        "mail property changes require a completed folder listing in this process; list the folder first",
    )
}

fn prepared_flag(
    folder: &str,
    server_id: &str,
    key: &str,
    patch: &MailPatch,
) -> Result<Option<Element>> {
    if !matches!(patch, MailPatch::Flag { .. }) {
        return Ok(None);
    }
    let root = decode(&build_mail_change(folder, server_id, key, patch)?)?
        .ok_or_else(|| AppError::new(ErrorCode::ProtocolError, "mail flag encoding is empty"))?;
    let flag = root.descendant("Email", "Flag").cloned().ok_or_else(|| {
        AppError::new(ErrorCode::ProtocolError, "mail flag encoding has no flag property")
    })?;
    Ok(Some(flag))
}

fn update_cached_property(
    collection: &mut CollectionState,
    server_id: &str,
    patch: &MailPatch,
    flag: Option<Element>,
) {
    if let Some(mail) = collection.mail.get_mut(server_id) {
        match patch {
            MailPatch::Read(read) => mail.is_read = Patch::Value(*read),
            MailPatch::Categories(categories) => mail.categories = Patch::Value(categories.clone()),
            MailPatch::Flag { .. } => mail.flag = flag.map_or(Patch::Missing, Patch::Value),
        }
    }
}
