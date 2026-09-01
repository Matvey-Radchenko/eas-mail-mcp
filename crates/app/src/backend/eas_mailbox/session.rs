use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use eas_mail_protocol::protocol::PolicyDecision;
use eas_mail_protocol::{
    CollectionKind, EasClient, EasError, Folder, HttpTransport, MailFields, ProfileRegistry,
    ServerCapabilities, Transport,
};
use tokio::sync::Mutex;

use super::super::{
    AccountBackend, BackendAccount, BackendCalendarMutation, BackendCalendarSearch,
    BackendCapabilities, BackendEvent, BackendMail, BackendSync, MailSource, OutgoingMail,
};
use super::VerificationStage;
use crate::config::AccountConfig;
use crate::keychain::{AccountSecret, SecretStore, StoredPolicy};
use crate::{AppError, ErrorCode, Result};

pub(super) struct CollectionState {
    pub(super) kind: CollectionKind,
    pub(super) sync_key: String,
    pub(super) mail: BTreeMap<String, MailFields>,
}

impl CollectionState {
    pub(super) fn new(kind: CollectionKind) -> Self {
        Self { kind, sync_key: "0".into(), mail: BTreeMap::new() }
    }
}

pub(super) struct SessionState {
    pub(super) capabilities: Option<ServerCapabilities>,
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
        let profile = profiles.require(&config.profile)?;
        let transport = HttpTransport::new(
            profile,
            config.username.clone(),
            secret.password.clone(),
            secret.device_id.clone(),
        )?;
        Self::with_transport_and_domains(
            account_id,
            config,
            secrets,
            Arc::new(transport),
            secret.policy_key,
            secret.policy.as_ref().map(PolicyDecision::from),
            profile.email_domains().to_vec(),
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
        let domains = config
            .email
            .rsplit_once('@')
            .map(|(_, domain)| vec![domain.to_ascii_lowercase()])
            .unwrap_or_default();
        Self::with_transport_and_domains(
            account_id, config, secrets, transport, policy_key, policy, domains,
        )
    }

    fn with_transport_and_domains(
        account_id: String,
        config: AccountConfig,
        secrets: Arc<dyn SecretStore>,
        transport: Arc<dyn Transport>,
        policy_key: u32,
        policy: Option<PolicyDecision>,
        email_domains: Vec<String>,
    ) -> Result<Self> {
        config.validate_shape()?;
        let account = BackendAccount {
            account_id,
            profile: config.profile.clone(),
            email: config.email,
            email_domains,
            enabled: config.enabled,
            write_enabled: config.write_enabled,
        };
        Ok(Self {
            account,
            client: Arc::new(EasClient::new(transport)),
            secrets,
            state: Mutex::new(SessionState {
                capabilities: None,
                policy_key,
                policy,
                folder_sync_key: "0".into(),
                folders: BTreeMap::new(),
                collections: BTreeMap::new(),
            }),
        })
    }

    pub(super) async fn ensure_ready(&self, state: &mut SessionState) -> Result<()> {
        if state.capabilities.is_none() {
            state.capabilities = Some(self.client.options().await.map_err(self.scoped_error())?);
        }
        if state.policy_key == 0 || state.policy.is_none() {
            self.refresh_policy(state).await?;
        }
        Ok(())
    }

    pub(super) fn require_capability(
        &self,
        state: &SessionState,
        command: eas_mail_protocol::Command,
    ) -> Result<()> {
        if state.capabilities.as_ref().is_some_and(|value| value.supports(command)) {
            Ok(())
        } else {
            Err(AppError::new(
                ErrorCode::ProtocolError,
                format!("Exchange does not advertise required command {}", command.name()),
            )
            .account(&self.account.account_id))
        }
    }

    pub(crate) async fn verification_result_with_progress(
        &self,
        progress: &mut dyn FnMut(VerificationStage) -> Result<()>,
    ) -> Result<(usize, bool)> {
        progress(VerificationStage::Transport)?;
        {
            let mut state = self.state.lock().await;
            if state.capabilities.is_none() {
                state.capabilities =
                    Some(self.client.options().await.map_err(self.scoped_error())?);
            }
            progress(VerificationStage::Capabilities)?;
            progress(VerificationStage::Policy)?;
            if state.policy_key == 0 || state.policy.is_none() {
                self.refresh_policy(&mut state).await?;
            }
        }
        progress(VerificationStage::FolderSync)?;
        let folders = self.refresh_folders().await?;
        let state = self.state.lock().await;
        let writes_supported =
            state.capabilities.as_ref().is_some_and(ServerCapabilities::supports_writes);
        Ok((folders.len(), writes_supported))
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

    async fn capabilities(&self) -> Result<BackendCapabilities> {
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        let capabilities = state.capabilities.as_ref().ok_or_else(|| {
            AppError::new(ErrorCode::ProtocolError, "Exchange capabilities are unavailable")
                .account(&self.account.account_id)
        })?;
        Ok(BackendCapabilities {
            calendar_availability: capabilities
                .supports(eas_mail_protocol::Command::ResolveRecipients),
            mail_writes: capabilities.supports_writes(),
            personal_calendar_writes: capabilities.supports_personal_calendar_writes(),
            meeting_lifecycle: capabilities.supports_meeting_lifecycle(),
        })
    }

    async fn folders(&self) -> Result<Vec<Folder>> {
        self.refresh_folders().await
    }

    async fn sync_mail(&self) -> Result<BackendSync> {
        self.sync_mail_selected(true, None).await
    }

    async fn list_mail(&self, folder_ids: Option<&[String]>) -> Result<Vec<BackendMail>> {
        let selected = match folder_ids {
            Some(values) => values.to_vec(),
            None => self.primary_mail_folder_ids().await?,
        };
        self.sync_mail_selected(false, Some(&selected)).await?;
        self.mail_snapshot(Some(&selected)).await
    }

    async fn search_mail(&self, query: &str, limit: usize) -> Result<Vec<BackendMail>> {
        self.search(query, limit).await
    }

    async fn search_people(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<eas_mail_protocol::protocol::DirectoryPage> {
        self.directory_search(query, limit).await
    }

    async fn fetch_mail(&self, source: &MailSource, body_limit: usize) -> Result<BackendMail> {
        self.fetch(source, body_limit).await
    }

    async fn fetch_attachment(&self, file_reference: &str) -> Result<Vec<u8>> {
        self.download(file_reference).await
    }

    async fn calendar_availability(
        &self,
        participants: &[String],
        starts_at: chrono::DateTime<chrono::Utc>,
        ends_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<eas_mail_protocol::RecipientAvailability>> {
        self.availability(participants, starts_at, ends_at).await
    }

    async fn search_calendar(&self, query: &str, limit: usize) -> Result<BackendCalendarSearch> {
        self.search_events(query, limit).await
    }

    async fn scan_calendar_metadata(&self) -> Result<BackendCalendarSearch> {
        self.scan_calendar_events().await
    }

    async fn fetch_calendar(
        &self,
        source: &BackendEvent,
        body_limit: usize,
    ) -> Result<BackendEvent> {
        self.fetch_event(source, body_limit).await
    }

    async fn resolve_calendar_source(&self, source: &BackendEvent) -> Result<BackendEvent> {
        self.mutable_event(source).await
    }

    async fn create_calendar_item(
        &self,
        client_id: &str,
        item: &BackendCalendarMutation,
    ) -> Result<BackendEvent> {
        self.add_event(client_id, item).await
    }

    async fn update_calendar_item(
        &self,
        source: &BackendEvent,
        item: &BackendCalendarMutation,
    ) -> Result<BackendEvent> {
        self.change_event(source, item).await
    }

    async fn delete_calendar_item(&self, source: &BackendEvent) -> Result<()> {
        self.delete_event(source).await
    }

    async fn respond_calendar_item(
        &self,
        source: &BackendEvent,
        response: eas_mail_protocol::MeetingResponseChoice,
    ) -> Result<Option<String>> {
        self.respond_event(source, response).await
    }

    async fn respond_meeting_request(
        &self,
        source: &MailSource,
        response: eas_mail_protocol::MeetingResponseChoice,
    ) -> Result<Option<String>> {
        self.respond_request(source, response).await
    }

    async fn send_calendar_message(&self, client_id: &str, mime: Vec<u8>) -> Result<()> {
        self.send_calendar_mime(client_id, mime).await
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
