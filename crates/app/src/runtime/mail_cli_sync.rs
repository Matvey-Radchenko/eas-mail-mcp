//! Folder reads requested explicitly by the CLI operator before a property write.

use std::collections::BTreeMap;

use crate::backend::{BackendMail, MailSource};
use crate::{AppError, ErrorCode, Result, Runtime};

impl Runtime {
    pub(crate) async fn check_cli_mail_property(&self, reference: &str) -> Result<()> {
        let mail = self.references.mail(reference)?;
        let backend = self.require_write(&mail.account_id)?;
        let source = match mail.source {
            source @ MailSource::Item { .. } => source,
            source @ MailSource::LongId(_) => {
                self.account_result(&mail.account_id, backend.resolve_mail_source(&source).await)?
                    .source
            }
        };
        self.account_result(&mail.account_id, backend.check_mail_property_ready(&source).await)
    }

    pub(crate) async fn sync_cli_mail_folders(&self, references: &[String]) -> Result<()> {
        let mut selected: BTreeMap<(String, String), Vec<BackendMail>> = BTreeMap::new();
        for reference in references {
            let mail = self.references.mail(reference)?;
            let backend = self.require_write(&mail.account_id)?;
            let before = self.account_result(
                &mail.account_id,
                backend.resolve_mail_source(&mail.source).await,
            )?;
            let MailSource::Item { folder_id, .. } = &before.source else {
                return Err(AppError::new(
                    ErrorCode::FeatureUnavailable,
                    "explicit folder synchronization requires an Item reference from mail_list",
                ));
            };
            selected.entry((mail.account_id, folder_id.clone())).or_default().push(before);
        }
        for ((account, folder), originals) in selected {
            let _guard = self.write_locks.acquire(&account).await?;
            let backend = self.require_write(&account)?;
            let snapshot =
                self.account_result(&account, backend.list_mail(Some(&[folder])).await)?;
            for before in originals {
                if !snapshot.iter().any(|mail| mail.source == before.source) {
                    return Err(changed(&account));
                }
                let after = self
                    .account_result(&account, backend.resolve_mail_source(&before.source).await)?;
                // Never find a replacement by subject. Both the locator and all available
                // point-read properties must remain identical across the explicit Sync.
                if before != after {
                    return Err(changed(&account));
                }
            }
        }
        Ok(())
    }
}

fn changed(account: &str) -> AppError {
    AppError::new(
        ErrorCode::SyncStale,
        "message changed during explicit folder synchronization; obtain a fresh reference and review again",
    )
    .account(account)
}

#[cfg(test)]
mod tests;
