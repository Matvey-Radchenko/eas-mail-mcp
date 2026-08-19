use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use eas_mail_protocol::protocol::PolicyDecision;
use eas_mail_protocol::{
    CalendarFields, CollectionKind, EasClient, EasError, Folder, HttpTransport, MailFields,
    ProfileRegistry, Transport,
};
use tokio::sync::Mutex;

use super::super::{
    AccountBackend, BackendAccount, BackendEvent, BackendMail, BackendSync, MailSource,
    OutgoingMail,
};
use crate::config::AccountConfig;
use crate::keychain::{AccountSecret, SecretStore, StoredPolicy};
use crate::{AppError, ErrorCode, Result};

pub(super) struct CollectionState {
    pub(super) kind: CollectionKind,
    pub(super) sync_key: String,
    pub(super) mail: BTreeMap<String, MailFields>,
    pub(super) calendar: BTreeMap<String, CalendarFields>,
}

impl CollectionState {
    pub(super) fn new(kind: CollectionKind) -> Self {
        Self { kind, sync_key: "0".into(), mail: BTreeMap::new(), calendar: BTreeMap::new() }
    }
}

pub(super) struct SessionState {
    pub(super) options_checked: bool,
    pub(super) policy_key: u32,
    pub(super) policy: Option<PolicyDecision>,
    pub(super) folder_sync_key: String,
    pub(super) folders: BTreeMap<String, Folder>,
    pub(super) collections: BTreeMap<String, CollectionState>,
}

/// EAS-backed, process-local account session.
pub struct EasMailbox {
    pub(super) account: BackendAccount,
    pub(super) client: Arc<EasClient>,
    pub(super) secrets: Arc<dyn SecretStore>,
    pub(super) state: Mutex<SessionState>,
}

impl EasMailbox {
    /// Creates a strict production mailbox for one fixed managed profile.
    pub fn production(
        account_id: String,
        config: AccountConfig,
        secrets: Arc<dyn SecretStore>,
        profiles: &ProfileRegistry,
    ) -> Result<Self> {
        let bundle = secrets.load()?;
        let secret = bundle.accounts.get(&account_id).cloned().ok_or_else(|| {
            AppError::new(ErrorCode::AuthRequired, "account credentials are missing")
                .account(&account_id)
        })?;
        Self::production_with_secret(account_id, config, secrets, secret, profiles)
    }

    pub(crate) fn production_with_secret(
        account_id: String,
        config: AccountConfig,
        secrets: Arc<dyn SecretStore>,
        secret: AccountSecret,
        profiles: &ProfileRegistry,
    ) -> Result<Self> {
        config.validate(profiles)?;
        let transport = HttpTransport::new(
            profiles.require(&config.profile)?,
            config.username.clone(),
            secret.password.clone(),
            secret.device_id.clone(),
        )?;
        Self::with_transport(
            account_id,
            config,
            secrets,
            Arc::new(transport),
            secret.policy_key,
            secret.policy.as_ref().map(PolicyDecision::from),
        )
    }

    /// Creates a mailbox over an injected transport for deterministic harnesses.
    pub fn with_transport(
        account_id: String,
        config: AccountConfig,
        secrets: Arc<dyn SecretStore>,
        transport: Arc<dyn Transport>,
        policy_key: u32,
        policy: Option<PolicyDecision>,
    ) -> Result<Self> {
        config.validate_shape()?;
        let account = BackendAccount {
            account_id,
            profile: config.profile.clone(),
            email: config.email,
            enabled: config.enabled,
            write_enabled: config.write_enabled,
        };
        Ok(Self {
            account,
            client: Arc::new(EasClient::new(transport)),
            secrets,
            state: Mutex::new(SessionState {
                options_checked: false,
                policy_key,
                policy,
                folder_sync_key: "0".into(),
                folders: BTreeMap::new(),
                collections: BTreeMap::new(),
            }),
        })
    }

    pub(super) async fn ensure_ready(&self, state: &mut SessionState) -> Result<()> {
        if !state.options_checked {
            self.client.options().await.map_err(self.scoped_error())?;
            state.options_checked = true;
        }
        if state.policy_key == 0 || state.policy.is_none() {
            self.refresh_policy(state).await?;
        }
        Ok(())
    }

    pub(super) async fn refresh_policy(&self, state: &mut SessionState) -> Result<()> {
        let negotiated = self.client.provision().await;
        match negotiated {
            Ok(policy) => {
                state.policy_key = policy.key;
                self.secrets.update(&mut |bundle| {
                    let secret =
                        bundle.accounts.get_mut(&self.account.account_id).ok_or_else(|| {
                            AppError::new(
                                ErrorCode::AuthRequired,
                                "account credentials are missing",
                            )
                            .account(&self.account.account_id)
                        })?;
                    secret.policy_key = policy.key;
                    secret.policy = Some(StoredPolicy::from(&policy.decision));
                    Ok(())
                })?;
                state.policy = Some(policy.decision);
                Ok(())
            }
            Err(EasError::AccountRemoteWipe) => {
                self.secrets.update(&mut |bundle| {
                    bundle.accounts.remove(&self.account.account_id);
                    Ok(())
                })?;
                state.folders.clear();
                state.collections.clear();
                state.policy_key = 0;
                state.policy = None;
                Err(AppError::from(EasError::AccountRemoteWipe).account(&self.account.account_id))
            }
            Err(error) => Err(AppError::from(error).account(&self.account.account_id)),
        }
    }

    pub(super) fn scoped_error(&self) -> impl FnOnce(EasError) -> AppError + '_ {
        |error| AppError::from(error).account(&self.account.account_id)
    }
}

#[async_trait]
impl AccountBackend for EasMailbox {
    fn account(&self) -> BackendAccount {
        self.account.clone()
    }

    async fn folders(&self) -> Result<Vec<Folder>> {
        self.refresh_folders().await
    }

    async fn sync(&self, mail: bool, calendar: bool) -> Result<BackendSync> {
        self.sync_selected(mail, calendar, true, None).await
    }

    async fn list_mail(&self, folder_ids: Option<&[String]>) -> Result<Vec<BackendMail>> {
        let selected = match folder_ids {
            Some(values) => values.to_vec(),
            None => self.primary_mail_folder_ids().await?,
        };
        self.sync_selected(true, false, false, Some(&selected)).await?;
        self.mail_snapshot(Some(&selected)).await
    }

    async fn search_mail(&self, query: &str, limit: usize) -> Result<Vec<BackendMail>> {
        self.search(query, limit).await
    }

    async fn fetch_mail(&self, source: &MailSource, body_limit: usize) -> Result<BackendMail> {
        self.fetch(source, body_limit).await
    }

    async fn fetch_attachment(&self, file_reference: &str) -> Result<Vec<u8>> {
        self.download(file_reference).await
    }

    async fn list_calendar(&self, folder_ids: Option<&[String]>) -> Result<Vec<BackendEvent>> {
        self.sync_selected(false, true, false, None).await?;
        self.calendar_snapshot(folder_ids).await
    }

    async fn mark_read(&self, source: &MailSource, is_read: bool) -> Result<()> {
        self.change_read(source, is_read).await
    }

    async fn send(&self, client_id: &str, message: &OutgoingMail) -> Result<()> {
        self.send_message(client_id, message).await
    }

    async fn reply(
        &self,
        client_id: &str,
        source: &MailSource,
        message: &OutgoingMail,
    ) -> Result<()> {
        self.compose(false, client_id, source, message).await
    }

    async fn forward(
        &self,
        client_id: &str,
        source: &MailSource,
        message: &OutgoingMail,
    ) -> Result<()> {
        self.compose(true, client_id, source, message).await
    }
}
