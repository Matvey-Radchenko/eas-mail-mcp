use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use eas_mail_protocol::wbxml::{Element, Node, encode};
use eas_mail_protocol::{Command, EasError, RequestSafety, Transport, TransportResponse};

#[derive(Clone)]
pub struct Call {
    pub safety: RequestSafety,
}

pub struct QueueTransport {
    options: Mutex<VecDeque<TransportResponse>>,
    commands: Mutex<VecDeque<TransportResponse>>,
    calls: Mutex<Vec<Call>>,
    purged: AtomicBool,
}

impl QueueTransport {
    pub fn with_options(value: TransportResponse) -> Self {
        Self {
            options: Mutex::new(VecDeque::from([value])),
            commands: Mutex::new(VecDeque::new()),
            calls: Mutex::new(Vec::new()),
            purged: AtomicBool::new(false),
        }
    }

    pub fn with_commands(values: Vec<TransportResponse>) -> Self {
        Self {
            options: Mutex::new(VecDeque::new()),
            commands: Mutex::new(VecDeque::from(values)),
            calls: Mutex::new(Vec::new()),
            purged: AtomicBool::new(false),
        }
    }

    pub fn calls(&self) -> eas_mail_protocol::Result<Vec<Call>> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .map_err(|_| EasError::Network("call lock poisoned".into()))
    }

    pub fn was_purged(&self) -> bool {
        self.purged.load(Ordering::Acquire)
    }

    fn next(
        queue: &Mutex<VecDeque<TransportResponse>>,
    ) -> eas_mail_protocol::Result<TransportResponse> {
        queue
            .lock()
            .map_err(|_| EasError::Network("response lock poisoned".into()))?
            .pop_front()
            .ok_or_else(|| EasError::Network("no scripted response".into()))
    }
}

#[async_trait]
impl Transport for QueueTransport {
    async fn options(&self) -> eas_mail_protocol::Result<TransportResponse> {
        Self::next(&self.options)
    }

    async fn command(
        &self,
        _: Command,
        _: &[u8],
        _: Option<u32>,
        safety: RequestSafety,
    ) -> eas_mail_protocol::Result<TransportResponse> {
        self.calls
            .lock()
            .map_err(|_| EasError::Network("call lock poisoned".into()))?
            .push(Call { safety });
        Self::next(&self.commands)
    }

    async fn purge_secrets(&self) {
        self.purged.store(true, Ordering::Release);
    }
}

pub fn boundary(transport: QueueTransport) -> Arc<dyn Transport> {
    Arc::new(transport)
}

pub fn response(
    status: u16,
    body: Vec<u8>,
    headers: BTreeMap<String, String>,
) -> TransportResponse {
    TransportResponse { status, body, headers }
}

pub fn headers() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("ms-asprotocolversions".into(), "12.1,14.1".into()),
        (
            "ms-asprotocolcommands".into(),
            "Provision,FolderSync,Sync,Search,ItemOperations,SendMail,SmartReply,SmartForward"
                .into(),
        ),
    ])
}

pub fn folder_response(status: u16, key: &str) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("FolderHierarchy", "FolderSync");
    root.push(Element::text("FolderHierarchy", "Status", status.to_string()));
    root.push(Element::text("FolderHierarchy", "SyncKey", key));
    encode(&root)
}

pub fn sync_response(status: u16, key: &str) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("AirSync", "Sync");
    let mut collection = Element::new("AirSync", "Collection");
    collection.push(Element::text("AirSync", "Status", status.to_string()));
    collection.push(Element::text("AirSync", "SyncKey", key));
    let mut collections = Element::new("AirSync", "Collections");
    collections.push(collection);
    root.push(collections);
    encode(&root)
}

pub fn search_empty() -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("Search", "Search");
    root.push(Element::text("Search", "Status", "1"));
    let mut response = Element::new("Search", "Response");
    let mut store = Element::new("Search", "Store");
    store.push(Element::text("Search", "Status", "1"));
    store.push(Element::text("Search", "Total", "0"));
    response.push(store);
    root.push(response);
    encode(&root)
}

pub fn item_response() -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("ItemOperations", "ItemOperations");
    let mut fetch = Element::new("ItemOperations", "Fetch");
    fetch.push(Element::text("ItemOperations", "Status", "1"));
    let mut properties = Element::new("ItemOperations", "Properties");
    properties.push(Element::text("Email", "Subject", "Full"));
    fetch.push(properties);
    root.push(fetch);
    encode(&root)
}

pub fn attachment_response() -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("ItemOperations", "ItemOperations");
    let mut fetch = Element::new("ItemOperations", "Fetch");
    fetch.push(Element::text("ItemOperations", "Status", "1"));
    let mut data = Element::new("ItemOperations", "Data");
    data.content.push(Node::Opaque(b"bytes".to_vec()));
    fetch.push(data);
    root.push(fetch);
    encode(&root)
}

pub fn mutation_response() -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("AirSync", "Sync");
    let mut collection = Element::new("AirSync", "Collection");
    collection.push(Element::text("AirSync", "CollectionId", "inbox"));
    collection.push(Element::text("AirSync", "Status", "1"));
    collection.push(Element::text("AirSync", "SyncKey", "10"));
    let mut collections = Element::new("AirSync", "Collections");
    collections.push(collection);
    root.push(collections);
    encode(&root)
}

pub fn compose_response(command: Command, status: u16) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("ComposeMail", command.name());
    root.push(Element::text("ComposeMail", "Status", status.to_string()));
    encode(&root)
}

pub fn provision_response(
    status: u16,
    key: Option<u32>,
    policy_status: Option<u16>,
    unsupported: bool,
) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("Provision", "Provision");
    root.push(Element::text("Provision", "Status", status.to_string()));
    let mut policy = Element::new("Provision", "Policy");
    if let Some(value) = key {
        policy.push(Element::text("Provision", "PolicyKey", value.to_string()));
    }
    if let Some(value) = policy_status {
        policy.push(Element::text("Provision", "Status", value.to_string()));
    }
    let mut document = Element::new("Provision", "EASProvisionDoc");
    if unsupported {
        document.push(Element::text("Provision", "DevicePasswordEnabled", "1"));
    }
    let mut data = Element::new("Provision", "Data");
    data.push(document);
    policy.push(data);
    let mut policies = Element::new("Provision", "Policies");
    policies.push(policy);
    root.push(policies);
    encode(&root)
}

pub fn wipe_response(account_only: bool) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("Provision", "Provision");
    root.push(Element::text("Provision", "Status", "1"));
    let mut policy = Element::new("Provision", "Policy");
    policy.push(Element::text("Provision", "PolicyKey", "77"));
    let mut policies = Element::new("Provision", "Policies");
    policies.push(policy);
    root.push(policies);
    root.push(Element::new(
        "Provision",
        if account_only { "AccountOnlyRemoteWipe" } else { "RemoteWipe" },
    ));
    encode(&root)
}
