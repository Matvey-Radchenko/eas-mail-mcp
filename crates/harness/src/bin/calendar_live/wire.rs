use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use eas_mail_mcp::backend::{AccountBackend, EasMailbox};
use eas_mail_mcp::{
    AppConfig, KeychainStore, Paths, RandomIds, Runtime, SecretStore, SqliteJournal, SystemClock,
};
use eas_mail_protocol::wbxml::{Element, decode};
use eas_mail_protocol::{
    Command, EasError, HttpTransport, ProfileRegistry, RequestSafety, Transport, TransportResponse,
};

pub fn runtime(
    config: &AppConfig,
    paths: &Paths,
    profiles: &ProfileRegistry,
    selected: Option<&str>,
) -> Result<Runtime> {
    paths.ensure()?;
    config.validate()?;
    let secrets: Arc<dyn SecretStore> = Arc::new(KeychainStore::new(paths.journal.clone()));
    let bundle = secrets.load()?;
    let mut backends: Vec<Arc<dyn AccountBackend>> = Vec::new();
    for (index, (id, account)) in config.accounts.iter().enumerate() {
        if !account.enabled || selected.is_some_and(|selected| selected != id) {
            continue;
        }
        account.validate(profiles)?;
        let secret = bundle.accounts.get(id).context("account credentials unavailable")?;
        let transport = Arc::new(ObservedTransport {
            account_index: index + 1,
            inner: HttpTransport::new(
                profiles.require(&account.profile)?,
                account.username.clone(),
                secret.password.clone(),
                secret.device_id.clone(),
            )?,
        });
        backends.push(Arc::new(EasMailbox::with_transport(
            id.clone(),
            account.clone(),
            secrets.clone(),
            transport,
            secret.policy_key,
            secret.policy.as_ref().map(Into::into),
        )?));
    }
    Ok(Runtime::with_dependencies(
        backends,
        Arc::new(SqliteJournal::open(&paths.journal)?),
        Arc::new(SystemClock),
        Arc::new(RandomIds),
        bundle.hmac_key.clone(),
        paths.attachments.clone(),
    )?)
}

struct ObservedTransport {
    account_index: usize,
    inner: HttpTransport,
}

#[async_trait]
impl Transport for ObservedTransport {
    async fn options(&self) -> eas_mail_protocol::Result<TransportResponse> {
        self.inner.options().await
    }

    async fn command(
        &self,
        command: Command,
        body: &[u8],
        key: Option<u32>,
        safety: RequestSafety,
    ) -> eas_mail_protocol::Result<TransportResponse> {
        if !matches!(
            command,
            Command::Sync
                | Command::FolderSync
                | Command::ItemOperations
                | Command::ResolveRecipients
                | Command::Provision
        ) || safety == RequestSafety::Mutation && command != Command::Sync
        {
            return Err(EasError::InvalidConfiguration(
                "calendar probe forbids this command".into(),
            ));
        }
        let request = decode(body).ok().flatten();
        if safety == RequestSafety::Mutation {
            let commands = request
                .as_ref()
                .and_then(|root| root.descendant("AirSync", "Commands"))
                .ok_or_else(|| {
                    EasError::InvalidConfiguration("calendar mutation has no commands".into())
                })?;
            if commands.children().count() != 1
                || commands.children().any(|item| {
                    !matches!(item.name.as_str(), "Add" | "Change" | "Delete")
                        || item.descendant("Email", "Read").is_some()
                        || item
                            .child("AirSync", "ApplicationData")
                            .is_some_and(|data| data.child("Calendar", "UID").is_none())
                })
            {
                return Err(EasError::InvalidConfiguration(
                    "calendar mutation guard rejected payload".into(),
                ));
            }
        }
        let response = self.inner.command(command, body, key, safety).await?;
        if safety == RequestSafety::Mutation {
            let mut request_tags = BTreeMap::new();
            let mut response_tags = BTreeMap::new();
            if let Some(request) = &request {
                collect(request, "", &mut request_tags);
            }
            if let Some(response) = decode(&response.body).ok().flatten() {
                collect(&response, "", &mut response_tags);
            }
            super::report(
                serde_json::json!({"account_index":self.account_index,"stage":"mutation_wire",
                "http_status":response.status,"request":request_tags,"response":response_tags}),
            )
            .map_err(|_| EasError::OutcomeUnknown)?;
        }
        Ok(response)
    }

    async fn purge_secrets(&self) {
        self.inner.purge_secrets().await;
    }
}

fn collect(node: &Element, parent: &str, tags: &mut BTreeMap<String, (usize, Vec<u16>)>) {
    let path = format!("{parent}/{}:{}", node.namespace, node.name);
    let value = tags.entry(path.clone()).or_default();
    value.0 += 1;
    if matches!(node.name.as_str(), "Status" | "Deleted")
        && let Ok(status) = node.text_content().parse::<u16>()
    {
        value.1.push(status);
    }
    for child in node.children() {
        collect(child, &path, tags);
    }
}
