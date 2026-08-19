use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use eas_mail_mcp::backend::{EasMailbox, OutgoingMail};
use eas_mail_mcp::{
    AccountConfig, AccountSecret, MemorySecretStore, SecretBundle, SecretStore, StoredPolicy,
};
use eas_mail_mcp_harness::{ExpectedCall, ScriptedTransport};
use eas_mail_protocol::protocol::{PolicyDecision, evaluate_policy};
use eas_mail_protocol::wbxml::{Element, Node, encode};
use eas_mail_protocol::{Command, ProfileKey, RequestSafety, Transport};

pub fn mailbox(
    calls: Vec<ExpectedCall>,
    policy: PolicyDecision,
) -> eas_mail_mcp::Result<(EasMailbox, Arc<ScriptedTransport>)> {
    let (mailbox, transport, _) = mailbox_with_store(calls, policy)?;
    Ok((mailbox, transport))
}

pub fn mailbox_with_store(
    calls: Vec<ExpectedCall>,
    policy: PolicyDecision,
) -> eas_mail_mcp::Result<(EasMailbox, Arc<ScriptedTransport>, Arc<MemorySecretStore>)> {
    let transport = Arc::new(ScriptedTransport::new(calls));
    let boundary: Arc<dyn Transport> = transport.clone();
    let secrets = Arc::new(secret_store(&policy));
    let secret_boundary: Arc<dyn SecretStore> = secrets.clone();
    let mailbox = EasMailbox::with_transport(
        "work".into(),
        account(),
        secret_boundary,
        boundary,
        123,
        Some(policy),
    )?;
    Ok((mailbox, transport, secrets))
}

pub fn default_policy() -> PolicyDecision {
    evaluate_policy(&BTreeMap::new())
}

pub fn policy(maximum: u64, attachments: bool) -> PolicyDecision {
    PolicyDecision {
        max_attachment_bytes: maximum,
        attachments_enabled: attachments,
        ..default_policy()
    }
}

pub fn options() -> ExpectedCall {
    ExpectedCall::Options {
        status: 200,
        headers: BTreeMap::from([
            ("ms-asprotocolversions".into(), "14.1".into()),
            (
                "ms-asprotocolcommands".into(),
                "Provision,FolderSync,Sync,Search,ItemOperations,SendMail,SmartReply,SmartForward"
                    .into(),
            ),
        ]),
    }
}

pub fn read(command: Command, body: Vec<u8>, response: Vec<u8>) -> ExpectedCall {
    call(command, body, Some(123), RequestSafety::RetrySafe, 200, response)
}

pub fn mutation(command: Command, body: Vec<u8>, response: Vec<u8>) -> ExpectedCall {
    call(command, body, Some(123), RequestSafety::Mutation, 200, response)
}

pub fn call(
    command: Command,
    body: Vec<u8>,
    policy_key: Option<u32>,
    safety: RequestSafety,
    status: u16,
    response: Vec<u8>,
) -> ExpectedCall {
    ExpectedCall::Command {
        command,
        body,
        policy_key,
        safety,
        status,
        response,
        delay: Duration::ZERO,
        failure: None,
    }
}

pub fn folder_response(key: &str, include_folders: bool) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("FolderHierarchy", "FolderSync");
    root.push(Element::text("FolderHierarchy", "Status", "1"));
    root.push(Element::text("FolderHierarchy", "SyncKey", key));
    if include_folders {
        let mut changes = Element::new("FolderHierarchy", "Changes");
        changes.push(folder("inbox", "Inbox", 2));
        changes.push(folder("calendar", "Calendar", 8));
        root.push(changes);
    }
    encode(&root)
}

pub fn sync_response(
    key: &str,
    status: u16,
    more: bool,
    commands: Vec<Element>,
) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("AirSync", "Sync");
    let mut collection = Element::new("AirSync", "Collection");
    collection.push(Element::text("AirSync", "SyncKey", key));
    collection.push(Element::text("AirSync", "Status", status.to_string()));
    if more {
        collection.push(Element::new("AirSync", "MoreAvailable"));
    }
    if !commands.is_empty() {
        let mut values = Element::new("AirSync", "Commands");
        for command in commands {
            values.push(command);
        }
        collection.push(values);
    }
    let mut collections = Element::new("AirSync", "Collections");
    collections.push(collection);
    root.push(collections);
    encode(&root)
}

pub fn mail_change(name: &str, id: &str, subject: Option<&str>) -> Element {
    let mut command = Element::new("AirSync", name);
    command.push(Element::text("AirSync", "ServerId", id));
    if let Some(subject) = subject {
        let mut data = Element::new("AirSync", "ApplicationData");
        data.push(Element::text("Email", "Subject", subject));
        data.push(Element::text("Email", "From", "sender@example.com"));
        data.push(Element::text("Email", "DateReceived", "20260102T030405Z"));
        data.push(Element::text("Email", "Read", "0"));
        command.push(data);
    }
    command
}

pub fn calendar_change(id: &str, subject: &str) -> Element {
    let mut command = Element::new("AirSync", "Add");
    command.push(Element::text("AirSync", "ServerId", id));
    let mut data = Element::new("AirSync", "ApplicationData");
    data.push(Element::text("Calendar", "Subject", subject));
    data.push(Element::text("Calendar", "StartTime", "20260102T030405Z"));
    data.push(Element::text("Calendar", "EndTime", "20260102T040405Z"));
    command.push(data);
    command
}

pub fn search_response() -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("Search", "Search");
    root.push(Element::text("Search", "Status", "1"));
    let mut result = Element::new("Search", "Result");
    result.push(Element::text("Search", "LongId", "long-1"));
    let mut properties = Element::new("Search", "Properties");
    properties.push(Element::text("Email", "Subject", "Found"));
    result.push(properties);
    root.push(result);
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

pub fn attachment_response(bytes: &[u8]) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("ItemOperations", "ItemOperations");
    let mut fetch = Element::new("ItemOperations", "Fetch");
    fetch.push(Element::text("ItemOperations", "Status", "1"));
    let mut data = Element::new("ItemOperations", "Data");
    data.content.push(Node::Opaque(bytes.to_vec()));
    fetch.push(data);
    root.push(fetch);
    encode(&root)
}

pub fn mutation_response(key: &str, status: u16) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("AirSync", "Sync");
    let mut collection = Element::new("AirSync", "Collection");
    collection.push(Element::text("AirSync", "SyncKey", key));
    let mut responses = Element::new("AirSync", "Responses");
    let mut change = Element::new("AirSync", "Change");
    change.push(Element::text("AirSync", "Status", status.to_string()));
    responses.push(change);
    collection.push(responses);
    let mut collections = Element::new("AirSync", "Collections");
    collections.push(collection);
    root.push(collections);
    encode(&root)
}

pub fn compose_response(status: u16) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("ComposeMail", "SendMail");
    root.push(Element::text("ComposeMail", "Status", status.to_string()));
    encode(&root)
}

pub fn provision_response(
    status: u16,
    key: Option<u32>,
    policy_status: Option<u16>,
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
    let mut data = Element::new("Provision", "Data");
    data.push(Element::new("Provision", "EASProvisionDoc"));
    policy.push(data);
    let mut policies = Element::new("Provision", "Policies");
    policies.push(policy);
    root.push(policies);
    encode(&root)
}

pub fn wipe_response() -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("Provision", "Provision");
    root.push(Element::text("Provision", "Status", "1"));
    let mut policy = Element::new("Provision", "Policy");
    policy.push(Element::text("Provision", "PolicyKey", "77"));
    let mut policies = Element::new("Provision", "Policies");
    policies.push(policy);
    root.push(policies);
    root.push(Element::new("Provision", "AccountOnlyRemoteWipe"));
    encode(&root)
}

pub fn outgoing() -> OutgoingMail {
    OutgoingMail {
        to: vec!["recipient@example.com".into()],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: "Subject".into(),
        body: "Body".into(),
    }
}

fn account() -> AccountConfig {
    AccountConfig {
        profile: ProfileKey::default(),
        email: "user@example.invalid".into(),
        username: "example_user".into(),
        enabled: true,
        write_enabled: true,
    }
}

fn secret_store(policy: &PolicyDecision) -> MemorySecretStore {
    let mut bundle = SecretBundle::new();
    bundle.accounts.insert(
        "work".into(),
        AccountSecret {
            password: "fixture-value".into(),
            device_id: "0011223344556677".into(),
            policy_key: 123,
            policy: Some(StoredPolicy::from(policy)),
        },
    );
    MemorySecretStore::with_bundle(bundle)
}

fn folder(id: &str, name: &str, folder_type: u16) -> Element {
    let mut add = Element::new("FolderHierarchy", "Add");
    add.push(Element::text("FolderHierarchy", "ServerId", id));
    add.push(Element::text("FolderHierarchy", "ParentId", "0"));
    add.push(Element::text("FolderHierarchy", "DisplayName", name));
    add.push(Element::text("FolderHierarchy", "Type", folder_type.to_string()));
    add
}
