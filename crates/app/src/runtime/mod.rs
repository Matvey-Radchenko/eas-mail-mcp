mod calendar;
mod convert;
mod outgoing;
mod reads;
mod writes;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eas_mail_protocol::{ProfileKey, ProfileRegistry};
use zeroize::Zeroize as _;

use crate::attachment_cache::AttachmentCache;
use crate::backend::{AccountBackend, EasMailbox};
use crate::model::{ApiResponse, SyncReport, Warning};
use crate::references::{Clock, IdGenerator, RandomIds, References, SystemClock};
use crate::{
    AppConfig, AppError, ErrorCode, ErrorEnvelope, KeychainStore, OperationJournal, Paths, Result,
    SecretStore, SqliteJournal,
};

/// Direct MCP application state shared by tool handlers.
pub struct Runtime {
    pub(super) backends: BTreeMap<String, Arc<dyn AccountBackend>>,
    pub(super) references: References,
    pub(super) journal: Arc<dyn OperationJournal>,
    pub(super) hmac_key: Vec<u8>,
    pub(super) attachments: AttachmentCache,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) sync_reports: Mutex<BTreeMap<String, SyncReport>>,
}

impl Runtime {
    /// Constructs a production runtime from standard per-user storage.
    pub fn production(
        config: AppConfig,
        paths: &Paths,
        profiles: &ProfileRegistry,
    ) -> Result<Self> {
        paths.ensure()?;
        config.validate_profiles(profiles)?;
        let journal: Arc<dyn OperationJournal> = Arc::new(SqliteJournal::open(&paths.journal)?);
        let _ = journal.prune()?;
        let secrets: Arc<dyn SecretStore> = Arc::new(KeychainStore::new(paths.journal.clone()));
        let bundle = secrets.load()?;
        let mut backends: Vec<Arc<dyn AccountBackend>> = Vec::new();
        for (account_id, account) in config.accounts {
            if account.enabled {
                let secret = bundle.accounts.get(&account_id).cloned().ok_or_else(|| {
                    AppError::new(ErrorCode::AuthRequired, "account credentials are missing")
                        .account(&account_id)
                })?;
                backends.push(Arc::new(EasMailbox::production_with_secret(
                    account_id,
                    account,
                    Arc::clone(&secrets),
                    secret,
                    profiles,
                )?));
            }
        }
        Self::with_dependencies(
            backends,
            journal,
            Arc::new(SystemClock),
            Arc::new(RandomIds),
            bundle.hmac_key.clone(),
            paths.attachments.clone(),
        )
    }

    pub(crate) fn purge_persisted_account(paths: &Paths, account_id: &str) -> Result<()> {
        paths.ensure()?;
        let journal = SqliteJournal::open(&paths.journal)?;
        let cache = AttachmentCache::new(paths.attachments.clone(), Arc::new(SystemClock))?;
        let journal_result = journal.purge_account(account_id).map(|_| ());
        let attachment_result = cache.purge_account(account_id);
        journal_result?;
        attachment_result
    }

    /// Constructs a runtime from explicit boundaries for the black-box harness.
    pub fn with_dependencies(
        backends: Vec<Arc<dyn AccountBackend>>,
        journal: Arc<dyn OperationJournal>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
        hmac_key: Vec<u8>,
        attachments_dir: PathBuf,
    ) -> Result<Self> {
        if hmac_key.len() != 32 {
            return Err(AppError::new(ErrorCode::StorageError, "journal HMAC key is invalid"));
        }
        let attachments = AttachmentCache::new(attachments_dir, Arc::clone(&clock))?;
        let mut indexed = BTreeMap::new();
        for backend in backends {
            let id = backend.account().account_id;
            if indexed.insert(id, backend).is_some() {
                return Err(AppError::new(
                    ErrorCode::ConfigInvalid,
                    "duplicate account identifier",
                ));
            }
        }
        Ok(Self {
            backends: indexed,
            references: References::new(clock.clone(), ids),
            journal,
            hmac_key,
            attachments,
            clock,
            sync_reports: Mutex::new(BTreeMap::new()),
        })
    }

    pub(super) fn selected(
        &self,
        requested: Option<&[String]>,
    ) -> Result<Vec<Arc<dyn AccountBackend>>> {
        if self.backends.is_empty() {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                "no enabled mail accounts are configured",
            ));
        }
        let ids = requested.map(|values| values.iter().collect::<BTreeSet<_>>());
        if let Some(ids) = &ids {
            for id in ids {
                if !self.backends.contains_key(id.as_str()) {
                    return Err(AppError::new(
                        ErrorCode::ValidationFailed,
                        "requested account is not configured or enabled",
                    )
                    .account((*id).clone()));
                }
            }
        }
        Ok(self
            .backends
            .iter()
            .filter(|(id, _)| ids.as_ref().is_none_or(|values| values.contains(id)))
            .map(|(_, backend)| Arc::clone(backend))
            .collect())
    }

    pub(super) fn backend(&self, account_id: &str) -> Result<Arc<dyn AccountBackend>> {
        self.backends.get(account_id).cloned().ok_or_else(|| {
            AppError::new(ErrorCode::NotFound, "account is not configured or enabled")
                .account(account_id)
        })
    }

    pub(super) fn require_write(&self, account_id: &str) -> Result<Arc<dyn AccountBackend>> {
        let backend = self.backend(account_id)?;
        if !backend.account().write_enabled {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                "write tools are disabled for this account",
            )
            .account(account_id));
        }
        Ok(backend)
    }

    pub(super) fn response<T>(result: Result<(T, Vec<Warning>)>) -> ApiResponse<T> {
        match result {
            Ok((data, warnings)) => ApiResponse::success(data, warnings),
            Err(error) => ApiResponse::failure(error.envelope),
        }
    }

    pub(super) fn collect_partial<T>(
        &self,
        results: Vec<(String, Result<T>)>,
    ) -> Result<(Vec<T>, Vec<Warning>)> {
        let mut values = Vec::new();
        let mut failures = Vec::new();
        for (account_id, result) in results {
            match result {
                Ok(value) => values.push(value),
                Err(error) => {
                    if error.envelope.code == ErrorCode::RemoteWipe {
                        self.purge_account(&account_id)?;
                    }
                    failures.push((account_id, error.envelope));
                }
            }
        }
        if values.is_empty()
            && let Some((_, envelope)) = failures.first()
        {
            return Err(AppError { envelope: envelope.clone() });
        }
        let warnings = failures.into_iter().map(warning).collect();
        Ok((values, warnings))
    }

    pub(super) fn account_result<T>(&self, account_id: &str, result: Result<T>) -> Result<T> {
        if result.as_ref().is_err_and(|error| error.envelope.code == ErrorCode::RemoteWipe) {
            self.purge_account(account_id)?;
        }
        result
    }

    fn purge_account(&self, account_id: &str) -> Result<()> {
        let references = self.references.purge_account(account_id);
        let journal = self.journal.purge_account(account_id).map(|_| ());
        let attachments = self.attachments.purge_account(account_id);
        references?;
        journal?;
        attachments
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.hmac_key.zeroize();
    }
}

fn warning((account_id, envelope): (String, ErrorEnvelope)) -> Warning {
    Warning {
        account_id,
        code: serde_json::to_value(envelope.code)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "PROTOCOL_ERROR".into()),
        message: envelope.message,
    }
}

pub(super) fn profile_name(profile: &ProfileKey) -> &str {
    profile.as_str()
}
