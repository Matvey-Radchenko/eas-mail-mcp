use std::collections::BTreeSet;

use eas_mail_protocol::{CollectionKind, EasError, MailFields, Patch};

use super::super::{BackendMail, BackendMailSearchPage, MailSource};
use super::session::{EasMailbox, SessionState};
use crate::{AppError, ErrorCode, Result};

impl EasMailbox {
    pub(super) async fn mail_snapshot(
        &self,
        folder_ids: Option<&[String]>,
    ) -> Result<Vec<BackendMail>> {
        let state = self.state.lock().await;
        let selected = selection(folder_ids);
        let mut output = Vec::new();
        for (folder_id, collection) in &state.collections {
            if collection.kind != CollectionKind::Mail || !included(&selected, folder_id) {
                continue;
            }
            output.extend(collection.mail.iter().map(|(server_id, fields)| BackendMail {
                account_id: self.account.account_id.clone(),
                folder_id: folder_id.clone(),
                source: MailSource::Item {
                    folder_id: folder_id.clone(),
                    server_id: server_id.clone(),
                },
                fields: fields.clone(),
            }));
        }
        output.sort_by_key(|item| std::cmp::Reverse(received(&item.fields)));
        Ok(output)
    }

    pub(super) async fn search(&self, query: &str, limit: usize) -> Result<Vec<BackendMail>> {
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        let preview_size = policy(&state)?.body_limit.min(500);
        let mut result = self.client.search(state.policy_key, query, 0, limit, preview_size).await;
        if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            let preview_size = policy(&state)?.body_limit.min(500);
            result = self.client.search(state.policy_key, query, 0, limit, preview_size).await;
        }
        Ok(result
            .map_err(self.scoped_error())?
            .into_iter()
            .map(|mail| search_mail(&self.account.account_id, mail))
            .collect())
    }

    pub(super) async fn search_page(
        &self,
        query: &eas_mail_protocol::MailSearchQuery,
        start: usize,
        limit: usize,
    ) -> Result<BackendMailSearchPage> {
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        let mut result = self
            .client
            .search_mail_page(
                state.policy_key,
                query,
                start,
                limit,
                policy(&state)?.body_limit.min(500),
            )
            .await;
        if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            result = self
                .client
                .search_mail_page(
                    state.policy_key,
                    query,
                    start,
                    limit,
                    policy(&state)?.body_limit.min(500),
                )
                .await;
        }
        let page = result.map_err(self.scoped_error())?;
        Ok(BackendMailSearchPage {
            server_truncated: page.server_truncated,
            items: page
                .items
                .into_iter()
                .map(|mail| search_mail(&self.account.account_id, mail))
                .collect(),
            total: page.total,
            range: page.range,
        })
    }

    pub(super) async fn fetch(
        &self,
        source: &MailSource,
        body_limit: usize,
    ) -> Result<BackendMail> {
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        let (long_id, folder_id, server_id) = source_parts(source);
        let body_limit = body_limit.min(policy(&state)?.body_limit);
        let mut result = self
            .client
            .fetch_item(state.policy_key, long_id, folder_id, server_id, body_limit)
            .await;
        if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            let body_limit = body_limit.min(policy(&state)?.body_limit);
            result = self
                .client
                .fetch_item(state.policy_key, long_id, folder_id, server_id, body_limit)
                .await;
        }
        let result = result.map_err(self.scoped_error())?;
        let folder_id = result.collection_id.as_deref().or(folder_id).unwrap_or_default();
        let source = match (result.collection_id.as_ref(), result.server_id.as_ref()) {
            (Some(folder), Some(server)) => {
                MailSource::Item { folder_id: folder.clone(), server_id: server.clone() }
            }
            _ => source.clone(),
        };
        Ok(BackendMail {
            account_id: self.account.account_id.clone(),
            folder_id: folder_id.to_owned(),
            source,
            fields: result.fields,
        })
    }

    pub(super) async fn download(&self, file_reference: &str) -> Result<Vec<u8>> {
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        let mut maximum = self.attachment_limit(&state)?;
        let mut result = self.client.fetch_attachment(state.policy_key, file_reference).await;
        if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            maximum = self.attachment_limit(&state)?;
            result = self.client.fetch_attachment(state.policy_key, file_reference).await;
        }
        let bytes = result.map_err(self.scoped_error())?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
            return Err(AppError::new(
                ErrorCode::PolicyBlocked,
                "attachment exceeds the Exchange policy limit",
            )
            .account(&self.account.account_id));
        }
        Ok(bytes)
    }

    fn attachment_limit(&self, state: &SessionState) -> Result<u64> {
        let policy = policy(state)?;
        if !policy.attachments_enabled {
            return Err(AppError::new(
                ErrorCode::PolicyBlocked,
                "Exchange policy disables attachment downloads",
            )
            .account(&self.account.account_id));
        }
        Ok(policy.max_attachment_bytes)
    }
}

fn search_mail(account_id: &str, mail: eas_mail_protocol::SearchMail) -> BackendMail {
    let source = match (&mail.collection_id, &mail.server_id) {
        (Some(folder), Some(server)) => {
            MailSource::Item { folder_id: folder.clone(), server_id: server.clone() }
        }
        _ => MailSource::LongId(mail.long_id),
    };
    BackendMail {
        account_id: account_id.into(),
        folder_id: mail.collection_id.unwrap_or_default(),
        source,
        fields: mail.fields,
    }
}

fn selection(values: Option<&[String]>) -> Option<BTreeSet<&str>> {
    values.map(|items| items.iter().map(String::as_str).collect())
}

fn included(selected: &Option<BTreeSet<&str>>, value: &str) -> bool {
    selected.as_ref().is_none_or(|items| items.contains(value))
}

fn received(fields: &MailFields) -> Option<chrono::DateTime<chrono::Utc>> {
    match &fields.received_at {
        Patch::Value(value) => *value,
        Patch::Missing => None,
    }
}

fn source_parts(source: &MailSource) -> (Option<&str>, Option<&str>, Option<&str>) {
    match source {
        MailSource::Item { folder_id, server_id } => {
            (None, Some(folder_id.as_str()), Some(server_id.as_str()))
        }
        MailSource::LongId(long_id) => (Some(long_id), None, None),
    }
}

fn policy(state: &SessionState) -> Result<&eas_mail_protocol::protocol::PolicyDecision> {
    state.policy.as_ref().ok_or_else(|| {
        AppError::new(ErrorCode::ProtocolError, "process-local Exchange state is inconsistent")
    })
}

#[cfg(test)]
mod tests;
