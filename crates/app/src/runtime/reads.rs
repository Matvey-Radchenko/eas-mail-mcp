use futures::future::join_all;

use super::convert::{folder_role, list};
use super::{Runtime, profile_name};
use crate::backend::BackendMail;
use crate::model::{
    AccountSelection, AccountStatus, AccountsData, AttachmentDownload, AttachmentDownloadInput,
    AttachmentView, AttachmentsData, FolderView, FoldersData, MailAttachmentsInput, MailDetail,
    MailGetInput, MailListInput, MailPage, MailSearchInput, SyncData, SyncReport,
};
use crate::references::AttachmentReference;
use crate::sanitize::{limit, safe_filename};
use crate::{ApiResponse, AppError, ErrorCode, Result};

const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;

impl Runtime {
    /// Lists safe metadata for every configured account.
    pub fn accounts_list(&self) -> ApiResponse<AccountsData> {
        let accounts = self
            .backends
            .values()
            .map(|backend| {
                let account = backend.account();
                AccountStatus {
                    account_id: account.account_id,
                    profile: profile_name(&account.profile).into(),
                    email: account.email,
                    enabled: account.enabled,
                    status: if !account.enabled {
                        "disabled"
                    } else if backend.configuration_error().is_some() {
                        "unavailable"
                    } else {
                        "unknown"
                    }
                    .into(),
                }
            })
            .collect();
        ApiResponse::success(AccountsData { accounts }, Vec::new())
    }

    /// Refreshes and lists folders, preserving partial account failures as warnings.
    pub async fn folders_list(&self, input: AccountSelection) -> ApiResponse<FoldersData> {
        Self::response(self.folders_result(input).await)
    }

    /// Returns process-local synchronization status without a network request.
    pub fn sync_status(&self, input: AccountSelection) -> ApiResponse<SyncData> {
        Self::response(self.sync_status_result(input))
    }

    /// Explicitly synchronizes selected mail collections over EAS.
    pub async fn sync_now(&self, input: AccountSelection) -> ApiResponse<SyncData> {
        Self::response(self.sync_result(input).await)
    }

    /// Performs a fresh list request or advances an immutable snapshot cursor.
    pub async fn mail_list(&self, input: MailListInput) -> ApiResponse<MailPage> {
        Self::response(self.mail_list_result(input).await)
    }

    /// Performs EAS Search or advances an immutable search snapshot cursor.
    pub async fn mail_search(&self, input: MailSearchInput) -> ApiResponse<MailPage> {
        Self::response(self.mail_search_result(input).await)
    }

    /// Fetches and sanitizes a full message body on demand.
    pub async fn mail_get(&self, input: MailGetInput) -> ApiResponse<MailDetail> {
        Self::response(self.mail_get_result(input).await)
    }

    /// Returns attachment metadata and portable opaque references.
    pub async fn mail_list_attachments(
        &self,
        input: MailAttachmentsInput,
    ) -> ApiResponse<AttachmentsData> {
        Self::response(self.attachments_result(input).await)
    }

    /// Downloads an attachment into the managed private cache.
    pub async fn mail_download_attachment(
        &self,
        input: AttachmentDownloadInput,
    ) -> ApiResponse<AttachmentDownload> {
        Self::response(self.download_result(input).await)
    }

    async fn folders_result(
        &self,
        input: AccountSelection,
    ) -> Result<(FoldersData, Vec<crate::Warning>)> {
        let backends = self.selected(input.account_ids.as_deref())?;
        let results = join_all(backends.into_iter().map(|backend| async move {
            let id = backend.account().account_id;
            let result = backend.folders().await.map(|folders| {
                folders
                    .into_iter()
                    .map(|folder| FolderView {
                        account_id: id.clone(),
                        folder_id: folder.server_id,
                        display_name: folder.display_name,
                        kind: folder
                            .kind
                            .map_or("other", |kind| match kind {
                                eas_mail_protocol::CollectionKind::Mail => "mail",
                                eas_mail_protocol::CollectionKind::Calendar => "calendar",
                            })
                            .into(),
                        role: folder_role(folder.folder_type).into(),
                        untrusted_external_content: true,
                    })
                    .collect::<Vec<_>>()
            });
            (id, result)
        }))
        .await;
        let (groups, warnings) = self.collect_partial(results)?;
        let folders = groups.into_iter().flatten().collect();
        Ok((FoldersData { folders }, warnings))
    }

    fn sync_status_result(
        &self,
        input: AccountSelection,
    ) -> Result<(SyncData, Vec<crate::Warning>)> {
        let selected = self.selected(input.account_ids.as_deref())?;
        let reports = self.sync_reports.lock().map_err(|_| state_error())?;
        let output = selected
            .iter()
            .filter_map(|backend| reports.get(&backend.account().account_id).cloned())
            .collect();
        Ok((SyncData { reports: output }, Vec::new()))
    }

    async fn sync_result(
        &self,
        input: AccountSelection,
    ) -> Result<(SyncData, Vec<crate::Warning>)> {
        let backends = self.selected(input.account_ids.as_deref())?;
        let completed_at = self.clock.now();
        let results = join_all(backends.into_iter().map(|backend| async move {
            let account_id = backend.account().account_id;
            let result = async {
                // Sync0 may reset server-side item bindings. Serialize explicit collection
                // synchronization with writes across all processes using this account.
                let _guard = self.write_locks.acquire(&account_id).await?;
                backend.sync_mail().await
            }
            .await
            .map(|value| SyncReport {
                account_id: account_id.clone(),
                scope: "mail".into(),
                collections_synced: value.collections,
                changes_applied: value.changes,
                completed_at,
            });
            (account_id, result)
        }))
        .await;
        let (reports, warnings) = self.collect_partial(results)?;
        let mut stored = self.sync_reports.lock().map_err(|_| state_error())?;
        for report in &reports {
            stored.insert(report.account_id.clone(), report.clone());
        }
        Ok((SyncData { reports }, warnings))
    }

    async fn mail_list_result(
        &self,
        input: MailListInput,
    ) -> Result<(MailPage, Vec<crate::Warning>)> {
        let page_limit = limit(input.limit.map(u32::from), 50, 100)?;
        if let Some(cursor) = input.cursor {
            return self.references.next_search_page(&cursor, page_limit);
        }
        let backends = self.selected(input.account_ids.as_deref())?;
        let folders = input.folder_ids;
        let results = join_all(backends.into_iter().map(|backend| {
            let folders = folders.clone();
            async move {
                let id = backend.account().account_id;
                let result = async {
                    let _guard = self.write_locks.acquire(&id).await?;
                    backend.list_mail(folders.as_deref()).await
                }
                .await;
                (id, result)
            }
        }))
        .await;
        let (groups, warnings) = self.collect_partial(results)?;
        let summaries = self.mail_summaries(groups.into_iter().flatten().collect())?;
        let (items, next_cursor) = self.references.first_mail_page(summaries, page_limit)?;
        Ok((
            MailPage { items, next_cursor, results_truncated: false, coverage: Vec::new() },
            warnings,
        ))
    }

    async fn mail_get_result(
        &self,
        input: MailGetInput,
    ) -> Result<(MailDetail, Vec<crate::Warning>)> {
        let body_limit = limit(input.body_limit, 12_000, 50_000)?;
        let source = self.references.mail(&input.mail_ref)?;
        let backend = self.backend(&source.account_id)?;
        let result = backend.fetch_mail(&source.source, body_limit).await;
        let mail = self.account_result(&source.account_id, result)?;
        Ok((self.mail_detail(input.mail_ref, &mail, body_limit), Vec::new()))
    }

    async fn attachments_result(
        &self,
        input: MailAttachmentsInput,
    ) -> Result<(AttachmentsData, Vec<crate::Warning>)> {
        let mut mail = self.references.mail(&input.mail_ref)?;
        if matches!(mail.fields.attachments, eas_mail_protocol::Patch::Missing) {
            let result = self.backend(&mail.account_id)?.fetch_mail(&mail.source, 12_000).await;
            mail = self.account_result(&mail.account_id, result)?;
        }
        let mut attachments = Vec::new();
        for item in list(&mail.fields.attachments) {
            let display_name = safe_filename(&item.display_name);
            let reference = AttachmentReference {
                account_id: mail.account_id.clone(),
                file_reference: item.file_reference,
                display_name: display_name.clone(),
            };
            let attachment_ref = self.references.insert_attachment(reference)?;
            attachments.push(AttachmentView {
                attachment_ref,
                account_id: mail.account_id.clone(),
                display_name,
                size: item.size,
                content_type: item.content_type,
                is_inline: item.is_inline,
                untrusted_external_content: true,
            });
        }
        Ok((AttachmentsData { attachments }, Vec::new()))
    }

    async fn download_result(
        &self,
        input: AttachmentDownloadInput,
    ) -> Result<(AttachmentDownload, Vec<crate::Warning>)> {
        let reference = self.references.attachment(&input.attachment_ref)?;
        let result =
            self.backend(&reference.account_id)?.fetch_attachment(&reference.file_reference).await;
        let bytes = self.account_result(&reference.account_id, result)?;
        if bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                "attachment exceeds the 25 MiB local download limit",
            ));
        }
        let token = self.references.next_token("file");
        let (path, expires_at) = self.attachments.store(
            &reference.account_id,
            &token,
            &reference.display_name,
            &bytes,
        )?;
        Ok((
            AttachmentDownload { path: path.to_string_lossy().into_owned(), expires_at },
            Vec::new(),
        ))
    }

    fn mail_summaries(&self, mail: Vec<BackendMail>) -> Result<Vec<crate::MailSummary>> {
        mail.into_iter().map(|item| self.mail_summary(item)).collect()
    }
}

fn state_error() -> AppError {
    AppError::new(ErrorCode::StorageError, "process-local runtime state is unavailable")
}
