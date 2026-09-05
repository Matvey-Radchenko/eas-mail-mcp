use super::{
    write_preview::{self, PreparedWrite, WritePreview},
    writes::existing_result,
};
use crate::backend::{BackendMail, MailSource};
use crate::model::{
    ApiResponse, MailAction, MailBatchItem, MailDeleteInput, MailMoveInput, MailMutationResult,
    MailSetCategoriesInput, MailSetFlagInput, OperationState,
};
use crate::{AppError, ErrorCode, MailResultLocator, OperationStatus, Result, Runtime};

impl Runtime {
    /// Moves one message to an existing folder and journals its resulting locator.
    pub async fn mail_move(&self, input: MailMoveInput) -> ApiResponse<MailMutationResult> {
        self.mail_mutation(
            MailBatchItem {
                mail_ref: input.mail_ref,
                idempotency_key: input.idempotency_key,
                action: MailAction::Move { destination_folder_id: input.destination_folder_id },
            },
            None,
        )
        .await
    }
    /// Moves one message into system trash; permanent deletion is unavailable.
    pub async fn mail_delete(&self, input: MailDeleteInput) -> ApiResponse<MailMutationResult> {
        self.mail_mutation(
            MailBatchItem {
                mail_ref: input.mail_ref,
                idempotency_key: input.idempotency_key,
                action: MailAction::Delete,
            },
            None,
        )
        .await
    }
    /// Changes one flag while preserving supported existing parameters.
    pub async fn mail_set_flag(&self, input: MailSetFlagInput) -> ApiResponse<MailMutationResult> {
        self.mail_mutation(
            MailBatchItem {
                mail_ref: input.mail_ref,
                idempotency_key: input.idempotency_key,
                action: MailAction::SetFlag { flag: input.flag },
            },
            None,
        )
        .await
    }
    /// Replaces the category set; an empty set clears it.
    pub async fn mail_set_categories(
        &self,
        input: MailSetCategoriesInput,
    ) -> ApiResponse<MailMutationResult> {
        self.mail_mutation(
            MailBatchItem {
                mail_ref: input.mail_ref,
                idempotency_key: input.idempotency_key,
                action: MailAction::SetCategories { categories: input.categories },
            },
            None,
        )
        .await
    }
    pub(crate) async fn mail_mutation(
        &self,
        input: MailBatchItem,
        expected: Option<&str>,
    ) -> ApiResponse<MailMutationResult> {
        Self::response(self.mutate_mail(&input, expected).await.map(|value| (value, Vec::new())))
    }

    pub(crate) async fn prepare_cli_mail_mutation(
        &self,
        input: &MailBatchItem,
    ) -> Result<PreparedWrite<MailMutationResult>> {
        validate(input)?;
        if let Some(result) = self.replay_mail_mutation(input)? {
            return Ok(PreparedWrite::Replay(result));
        }
        let source = self.references.mail(&input.mail_ref)?;
        let backend = self.require_write(&source.account_id)?;
        let mail = self.account_result(
            &source.account_id,
            backend.resolve_mail_source(&source.source).await,
        )?;
        let destination =
            self.account_result(&source.account_id, destination(&*backend, &input.action).await)?;
        Ok(PreparedWrite::Ready(preview(input, &mail, destination.as_deref())?))
    }

    async fn mutate_mail(
        &self,
        input: &MailBatchItem,
        expected: Option<&str>,
    ) -> Result<MailMutationResult> {
        validate(input)?;
        if let Some(result) = self.replay_mail_mutation(input)? {
            return Ok(result);
        }
        let source = self.references.mail(&input.mail_ref)?;
        self.require_write(&source.account_id)?;
        let _guard = self.write_locks.acquire(&source.account_id).await?;
        let backend = self.require_write(&source.account_id)?;
        if let Some(result) = self.replay_mail_mutation(input)? {
            return Ok(result);
        }
        let mut mail = self.account_result(
            &source.account_id,
            backend.resolve_mail_source(&source.source).await,
        )?;
        let destination =
            self.account_result(&source.account_id, destination(&*backend, &input.action).await)?;
        write_preview::verify(&preview(input, &mail, destination.as_deref())?, expected)?;
        if !matches!(input.action, MailAction::Move { .. } | MailAction::Delete) {
            self.account_result(
                &source.account_id,
                backend.check_mail_property_ready(&mail.source).await,
            )?;
        }
        let begin =
            self.begin_write(&mail.account_id, input.kind(), &input.idempotency_key, input)?;
        if !begin.inserted {
            return self
                .replay_mail_mutation(input)?
                .ok_or_else(|| AppError::new(ErrorCode::StorageError, "operation disappeared"));
        }
        let result = match &input.action {
            MailAction::Move { .. } | MailAction::Delete => {
                let target = destination.as_deref().ok_or_else(|| {
                    AppError::new(ErrorCode::ValidationFailed, "missing destination")
                })?;
                backend.move_mail(&mail.source, target).await
            }
            MailAction::MarkRead { is_read } => {
                backend.mark_read(&mail.source, *is_read).await.map(|()| mail.source.clone())
            }
            MailAction::SetFlag { flag } => {
                backend.set_mail_flag(&mail.source, flag.eas()).await.map(|()| mail.source.clone())
            }
            MailAction::SetCategories { categories } => backend
                .set_mail_categories(&mail.source, categories)
                .await
                .map(|()| mail.source.clone()),
        };
        self.finish_mail_mutation(&begin.record.operation_id, &mut mail, result)
    }

    fn finish_mail_mutation(
        &self,
        operation_id: &str,
        mail: &mut BackendMail,
        result: Result<MailSource>,
    ) -> Result<MailMutationResult> {
        let source = match result {
            Ok(source) => source,
            Err(error) if error.envelope.code == ErrorCode::RemoteWipe => {
                Self::journal_after_mutation(
                    self.purge_account(&mail.account_id),
                    &mail.account_id,
                    operation_id,
                )?;
                return Err(error.account(&mail.account_id).operation(operation_id));
            }
            Err(error) => {
                let state = if error.envelope.code == ErrorCode::OutcomeUnknown {
                    OperationStatus::Unknown
                } else {
                    OperationStatus::Failed
                };
                self.journal.finish(operation_id, state, 0).map_err(|_| unknown(operation_id))?;
                return Err(error.account(&mail.account_id).operation(operation_id));
            }
        };
        let MailSource::Item { folder_id, server_id } = &source else {
            return Err(unknown(operation_id));
        };
        let locator =
            MailResultLocator { folder_id: folder_id.clone(), server_id: server_id.clone() };
        self.journal
            .finish_with_locator(operation_id, OperationStatus::Succeeded, 1, Some(&locator))
            .map_err(|_| unknown(operation_id))?;
        mail.folder_id = folder_id.clone();
        mail.source = source;
        let mail_ref =
            self.references.insert_mail(mail.clone()).map_err(|_| unknown(operation_id))?;
        Ok(MailMutationResult {
            operation_id: operation_id.into(),
            status: OperationState::Succeeded,
            message: "Exchange confirmed the operation".into(),
            mail_ref: Some(mail_ref),
        })
    }

    fn replay_mail_mutation(&self, input: &MailBatchItem) -> Result<Option<MailMutationResult>> {
        let Some(record) = self.replay_write(input.kind(), &input.idempotency_key, input)? else {
            return Ok(None);
        };
        if matches!(record.status, OperationStatus::Pending | OperationStatus::Unknown) {
            return Err(unknown(&record.operation_id));
        }
        let entry = self
            .journal
            .inspect(&record.operation_id)?
            .ok_or_else(|| AppError::new(ErrorCode::StorageError, "operation disappeared"))?;
        let mail_ref = if let Some(locator) = entry.result_locator {
            Some(self.references.insert_mail(BackendMail {
                account_id: record.account_id.clone(),
                folder_id: locator.folder_id.clone(),
                source: MailSource::Item {
                    folder_id: locator.folder_id,
                    server_id: locator.server_id,
                },
                fields: Default::default(),
            })?)
        } else {
            None
        };
        let result = existing_result(record);
        Ok(Some(MailMutationResult {
            operation_id: result.operation_id,
            status: result.status,
            message: result.message,
            mail_ref,
        }))
    }
}

pub(super) fn validate(input: &MailBatchItem) -> Result<()> {
    uuid::Uuid::parse_str(&input.idempotency_key).map_err(|_| {
        AppError::new(ErrorCode::ValidationFailed, "idempotency_key must be a UUID")
    })?;
    if let MailAction::SetCategories { categories } = &input.action {
        let unique = categories.iter().collect::<std::collections::BTreeSet<_>>();
        if categories.len() > 50
            || unique.len() != categories.len()
            || categories.iter().any(|name| {
                name.trim().is_empty()
                    || name.chars().count() > 255
                    || name.chars().any(char::is_control)
            })
        {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                "categories require up to 50 unique names of 1–255 characters without control characters",
            ));
        }
    }
    Ok(())
}

async fn destination(
    backend: &dyn crate::backend::AccountBackend,
    action: &MailAction,
) -> Result<Option<String>> {
    if !matches!(action, MailAction::Move { .. } | MailAction::Delete) {
        return Ok(None);
    }
    let folders = backend.folders().await?;
    let folder = match action {
        MailAction::Move { destination_folder_id } => {
            folders.iter().find(|folder| &folder.server_id == destination_folder_id)
        }
        MailAction::Delete => folders.iter().find(|folder| folder.folder_type == 4),
        _ => None,
    }
    .filter(|folder| folder.kind == Some(eas_mail_protocol::CollectionKind::Mail))
    .ok_or_else(|| {
        AppError::new(
            ErrorCode::FeatureUnavailable,
            "the existing destination mail folder is unavailable",
        )
    })?;
    Ok(Some(folder.server_id.clone()))
}

fn preview(
    input: &MailBatchItem,
    mail: &BackendMail,
    destination: Option<&str>,
) -> Result<WritePreview> {
    Ok(WritePreview::new(input.kind(), mail.account_id.clone())
        .field("Sender", super::convert::string(&mail.fields.sender))
        .field("Subject", super::convert::string(&mail.fields.subject))
        .field("Source folder", &mail.folder_id)
        .field("Destination folder", destination.unwrap_or(""))
        .field(
            "Action",
            serde_json::to_string(&input.action)
                .map_err(|_| AppError::new(ErrorCode::ValidationFailed, "invalid mail action"))?,
        )
        .field("Existing flag", format!("{:?}", mail.fields.flag))
        .field("Existing categories", format!("{:?}", mail.fields.categories)))
}

fn unknown(operation_id: &str) -> AppError {
    AppError::new(
        ErrorCode::OutcomeUnknown,
        "operation outcome is unknown; inspect the UUID and do not repeat it with a new UUID",
    )
    .operation(operation_id)
}
