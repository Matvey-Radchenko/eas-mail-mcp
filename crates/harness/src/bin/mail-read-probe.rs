use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write as _};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use clap::Parser;
use eas_mail_mcp::backend::{AccountBackend, EasMailbox, MailSource};
use eas_mail_mcp::{
    KeychainStore, MailGetThreadInput, MailSearchInput, MemorySecretStore, Paths, RandomIds,
    Runtime, SecretStore, SystemClock, load_config, load_profile_registry,
};
use eas_mail_mcp_harness::MemoryJournal;
use eas_mail_protocol::wbxml::{Element, Node, decode};
use eas_mail_protocol::{
    Command, EasError, HttpTransport, MailSearchQuery, RequestSafety, Transport, TransportResponse,
};
use serde::Serialize;

#[path = "mail_read_probe/point_sync.rs"]
mod point_sync;

#[derive(Parser)]
struct Arguments {
    #[arg(long)]
    account: String,
    #[arg(long, default_value = "EAS Mail MCP")]
    query: String,
    /// Inspect only FolderSync and an initial Inbox SyncKey request, with no GetChanges or writes.
    #[arg(long)]
    initial_sync: bool,
    /// Read a private locator JSON using ItemOperations, then initial Sync and exact Sync Fetch.
    #[arg(long, conflicts_with = "initial_sync")]
    point_sync: Option<std::path::PathBuf>,
}

#[derive(Default, Serialize)]
struct Shape {
    count: usize,
    numeric_values: BTreeSet<String>,
    opaque_lengths: BTreeSet<usize>,
}

#[derive(Serialize)]
struct WireReport {
    command: String,
    http_status: u16,
    body_bytes: usize,
    decoded: bool,
    tags: BTreeMap<String, Shape>,
}

struct ObservedTransport {
    inner: HttpTransport,
    reports: Mutex<Vec<WireReport>>,
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
        // This diagnostic permits only point reads; no folder synchronization or mutation.
        if !matches!(command, Command::Search | Command::ItemOperations)
            || safety != RequestSafety::RetrySafe
        {
            return Err(EasError::InvalidConfiguration("read probe forbids this command".into()));
        }
        let response = self.inner.command(command, body, key, safety).await?;
        let tree = decode(&response.body).ok().flatten();
        let mut tags = BTreeMap::new();
        if let Some(tree) = &tree {
            collect_shape(tree, "", &mut tags);
        }
        self.reports.lock().map_err(|_| EasError::Protocol("diagnostic lock failed".into()))?.push(
            WireReport {
                command: format!("{command:?}"),
                http_status: response.status,
                body_bytes: response.body.len(),
                decoded: tree.is_some(),
                tags,
            },
        );
        Ok(response)
    }

    async fn purge_secrets(&self) {
        self.inner.purge_secrets().await;
    }
}

fn collect_shape(element: &Element, parent: &str, tags: &mut BTreeMap<String, Shape>) {
    let path = format!("{parent}/{}:{}", element.namespace, element.name);
    let shape = tags.entry(path.clone()).or_default();
    shape.count += 1;
    if matches!(element.name.as_str(), "Status" | "Range" | "Total") {
        let value = element.text_content();
        if value.len() <= 24 && value.bytes().all(|byte| byte.is_ascii_digit() || byte == b'-') {
            shape.numeric_values.insert(value);
        }
    }
    if element.name == "ConversationId" {
        for node in &element.content {
            if let Node::Opaque(value) = node {
                shape.opaque_lengths.insert(value.len());
            }
        }
    }
    for child in element.children() {
        collect_shape(child, &path, tags);
    }
}

#[derive(Default, Serialize)]
struct Report {
    swapped_search_error: Option<String>,
    swapped_search_items: usize,
    swapped_conversations_verified: bool,
    opaque_search_error: Option<String>,
    opaque_search_items: usize,
    opaque_conversations_verified: bool,
    search_error: Option<String>,
    search_candidates: usize,
    search_seed_has_mutable_ids: bool,
    point_error: Option<String>,
    point_has_mutable_ids: bool,
    thread_error: Option<String>,
    thread_items: usize,
    thread_truncated: bool,
    wire: Vec<WireReport>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Arguments::parse();
    let paths = Paths::standard()?;
    let profiles = load_profile_registry(&paths.profiles)?
        .ok_or_else(|| anyhow::anyhow!("profiles unavailable"))?;
    let config = load_config(&paths.config)?;
    let account = config
        .accounts
        .get(&args.account)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("account unavailable"))?;
    let bundle = KeychainStore::new(paths.journal.clone()).load()?;
    let secret = bundle
        .accounts
        .get(&args.account)
        .ok_or_else(|| anyhow::anyhow!("account credentials unavailable"))?;
    anyhow::ensure!(
        secret.policy_key != 0 && secret.policy.is_some(),
        "probe requires an already provisioned account"
    );
    if args.initial_sync {
        return initial_sync_probe(&account, secret, &profiles).await;
    }
    if let Some(locator) = &args.point_sync {
        return point_sync::run(&account, secret, &profiles, locator).await;
    }
    let transport = Arc::new(ObservedTransport {
        inner: HttpTransport::new(
            profiles.require(&account.profile)?,
            account.username.clone(),
            secret.password.clone(),
            secret.device_id.clone(),
        )?,
        reports: Mutex::new(Vec::new()),
    });
    let memory = Arc::new(MemorySecretStore::with_bundle(bundle.clone()));
    let mailbox = Arc::new(EasMailbox::with_transport(
        args.account.clone(),
        account,
        memory,
        transport.clone(),
        secret.policy_key,
        secret.policy.as_ref().map(Into::into),
    )?);
    let mut report = Report::default();
    let query = MailSearchQuery { text: args.query.clone(), ..Default::default() };
    match mailbox.search_mail_page(&query, 0, 100).await {
        Ok(page) => {
            report.search_candidates = page.items.len();
            let seed = page
                .items
                .iter()
                .filter(|mail| {
                    matches!(&mail.fields.message_class,
                eas_mail_protocol::Patch::Value(value) if value == "IPM.Note")
                })
                .max_by_key(|mail| {
                    page.items
                        .iter()
                        .filter(|other| other.fields.conversation_id == mail.fields.conversation_id)
                        .count()
                })
                .or_else(|| page.items.first());
            if let Some(seed) = seed {
                if let eas_mail_protocol::Patch::Value(id) = &seed.fields.conversation_id {
                    match opaque_search(transport.as_ref(), secret.policy_key, id, false).await {
                        Ok((count, verified)) => {
                            report.opaque_search_items = count;
                            report.opaque_conversations_verified = verified;
                        }
                        Err(_) => report.opaque_search_error = Some("PROTOCOL_ERROR".into()),
                    }
                    match opaque_search(transport.as_ref(), secret.policy_key, id, true).await {
                        Ok((count, verified)) => {
                            report.swapped_search_items = count;
                            report.swapped_conversations_verified = verified;
                        }
                        Err(_) => report.swapped_search_error = Some("PROTOCOL_ERROR".into()),
                    }
                }
                report.search_seed_has_mutable_ids = matches!(seed.source, MailSource::Item { .. });
                match mailbox.resolve_mail_source(&seed.source).await {
                    Ok(mail) => {
                        report.point_has_mutable_ids =
                            matches!(mail.source, MailSource::Item { .. })
                    }
                    Err(error) => report.point_error = Some(error.envelope.code.as_str().into()),
                }
                check_thread(mailbox, &args, &bundle.hmac_key, &mut report).await?;
            }
        }
        Err(error) => report.search_error = Some(error.envelope.code.as_str().into()),
    }
    report.wire = std::mem::take(
        &mut *transport.reports.lock().map_err(|_| anyhow::anyhow!("diagnostic lock failed"))?,
    );
    serde_json::to_writer_pretty(io::stdout(), &report)?;
    writeln!(io::stdout())?;
    Ok(())
}

async fn initial_sync_probe(
    account: &eas_mail_mcp::AccountConfig,
    secret: &eas_mail_mcp::AccountSecret,
    profiles: &eas_mail_protocol::ProfileRegistry,
) -> anyhow::Result<()> {
    let transport = Arc::new(HttpTransport::new(
        profiles.require(&account.profile)?,
        account.username.clone(),
        secret.password.clone(),
        secret.device_id.clone(),
    )?);
    let client = eas_mail_protocol::EasClient::new(transport.clone());
    let folders = client.folder_sync(secret.policy_key, "0").await?;
    let inbox = folders
        .folders
        .iter()
        .find(|folder| folder.folder_type == 2)
        .ok_or_else(|| anyhow::anyhow!("system Inbox unavailable"))?;
    let body = eas_mail_protocol::protocol::build_sync(
        &inbox.server_id,
        "0",
        eas_mail_protocol::CollectionKind::Mail,
        0,
        0,
    )?;
    let request = decode(&body)?.ok_or_else(|| anyhow::anyhow!("missing Sync request"))?;
    anyhow::ensure!(
        request.descendant("AirSync", "Commands").is_none()
            && request.descendant("AirSync", "GetChanges").is_none(),
        "probe cannot fetch mail or mutate it"
    );
    let response = transport
        .command(Command::Sync, &body, Some(secret.policy_key), RequestSafety::RetrySafe)
        .await?;
    let tree = decode(&response.body).ok().flatten();
    let mut tags = BTreeMap::new();
    if let Some(tree) = &tree {
        collect_shape(tree, "", &mut tags);
    }
    let report = WireReport {
        command: "InitialSyncOnly".into(),
        http_status: response.status,
        body_bytes: response.body.len(),
        decoded: tree.is_some(),
        tags,
    };
    serde_json::to_writer_pretty(io::stdout(), &report)?;
    writeln!(io::stdout())?;
    Ok(())
}

async fn opaque_search(
    transport: &dyn Transport,
    key: u32,
    id: &[u8],
    swap: bool,
) -> anyhow::Result<(usize, bool)> {
    let bytes =
        eas_mail_protocol::protocol::build_mail_search(&MailSearchQuery::default(), 0, 3, 1)?;
    let mut root = decode(&bytes)?.ok_or_else(|| anyhow::anyhow!("missing search request"))?;
    let mut guid = id.to_vec();
    if swap {
        for range in [0..4, 4..6, 6..8] {
            if let Some(field) = guid.get_mut(range) {
                field.reverse();
            }
        }
    }
    add_conversation(&mut root, &guid);
    let response = transport
        .command(
            Command::Search,
            &eas_mail_protocol::wbxml::encode(&root)?,
            Some(key),
            RequestSafety::RetrySafe,
        )
        .await?;
    let page = eas_mail_protocol::protocol::parse_mail_search(&response.body)?;
    let verified = !page.items.is_empty()
        && page.items.iter().all(|item| {
            item.fields.conversation_id == eas_mail_protocol::Patch::Value(id.to_vec())
        });
    Ok((page.items.len(), verified))
}

fn add_conversation(element: &mut Element, id: &[u8]) {
    if element.namespace == "Search" && element.name == "And" {
        let mut conversation = Element::new("Search", "ConversationId");
        conversation.content.push(Node::Opaque(id.to_vec()));
        element.push(conversation);
        return;
    }
    for node in &mut element.content {
        if let Node::Element(child) = node {
            add_conversation(child, id);
        }
    }
}

async fn check_thread(
    mailbox: Arc<EasMailbox>,
    args: &Arguments,
    hmac: &[u8],
    report: &mut Report,
) -> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let runtime = Runtime::with_dependencies(
        vec![mailbox],
        Arc::new(MemoryJournal::default()),
        Arc::new(SystemClock),
        Arc::new(RandomIds),
        hmac.to_vec(),
        temporary.path().join("attachments"),
    )?;
    let search = runtime
        .mail_search(MailSearchInput {
            query: args.query.clone(),
            limit: Some(1),
            ..Default::default()
        })
        .await;
    let Some(seed) = search.data.and_then(|data| data.items.into_iter().next()) else {
        report.thread_error = search.error.map(|error| error.code.as_str().into());
        return Ok(());
    };
    let thread = runtime
        .mail_get_thread(MailGetThreadInput {
            mail_ref: seed.mail_ref,
            limit: Some(3),
            body_limit: Some(1),
            total_body_limit: Some(3),
        })
        .await;
    report.thread_error = thread.error.map(|error| error.code.as_str().into());
    if let Some(data) = thread.data {
        report.thread_items = data.items.len();
        report.thread_truncated = data.results_truncated;
    }
    Ok(())
}
