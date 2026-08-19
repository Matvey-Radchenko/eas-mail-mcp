use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use eas_mail_mcp::backend::{AccountBackend as _, EasMailbox};
use eas_mail_mcp::{AccountConfig, AccountSecret, MemorySecretStore, SecretBundle, SecretStore};
use eas_mail_mcp_harness::{ExpectedCall, ScriptedFailure, ScriptedTransport};
use eas_mail_protocol::protocol::{
    build_folder_sync, build_initial_provision, build_search, build_send, build_sync,
    build_wipe_ack, evaluate_policy,
};
use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{
    CollectionKind, Command, EasClient, EasError, ProfileKey, RequestSafety, Transport,
};

#[tokio::test]
async fn empty_more_available_page_is_followed_until_last_page() -> anyhow::Result<()> {
    let calls = vec![
        ExpectedCall::Options { status: 200, headers: options_headers() },
        expected(Command::FolderSync, build_folder_sync("0")?, folder_response()?),
        expected(
            Command::Sync,
            build_sync("inbox", "0", CollectionKind::Mail, 5, 500)?,
            sync_response("10", false, false)?,
        ),
        expected(
            Command::Sync,
            build_sync("inbox", "10", CollectionKind::Mail, 5, 500)?,
            sync_response("11", true, false)?,
        ),
        expected(
            Command::Sync,
            build_sync("inbox", "11", CollectionKind::Mail, 5, 500)?,
            sync_response("12", false, true)?,
        ),
    ];
    let transport = Arc::new(ScriptedTransport::new(calls));
    let transport_boundary: Arc<dyn Transport> = transport.clone();
    let secrets: Arc<dyn SecretStore> = Arc::new(secret_store());
    let mailbox = EasMailbox::with_transport(
        "example".into(),
        account(),
        secrets,
        transport_boundary,
        123,
        Some(evaluate_policy(&BTreeMap::new())),
    )?;
    let mail = mailbox.list_mail(None).await?;
    anyhow::ensure!(mail.len() == 1, "expected one synchronized message");
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn ambiguous_write_is_never_retried() -> anyhow::Result<()> {
    let body = build_send("client-1", b"mime".to_vec())?;
    let transport = Arc::new(ScriptedTransport::new(vec![ExpectedCall::Command {
        command: Command::SendMail,
        body,
        policy_key: Some(123),
        safety: RequestSafety::Mutation,
        status: 0,
        response: Vec::new(),
        delay: Duration::ZERO,
        failure: Some(ScriptedFailure::OutcomeUnknown),
    }]));
    let boundary: Arc<dyn Transport> = transport.clone();
    let client = EasClient::new(boundary);
    let error = client.send(123, "client-1", b"mime".to_vec()).await;
    anyhow::ensure!(matches!(error, Err(EasError::OutcomeUnknown)));
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn scripted_delay_can_be_cancelled_by_a_timeout() -> anyhow::Result<()> {
    let body = build_search("query", 0, 10, 500)?;
    let transport = Arc::new(ScriptedTransport::new(vec![ExpectedCall::Command {
        command: Command::Search,
        body,
        policy_key: Some(123),
        safety: RequestSafety::RetrySafe,
        status: 200,
        response: Vec::new(),
        delay: Duration::from_millis(100),
        failure: None,
    }]));
    let boundary: Arc<dyn Transport> = transport.clone();
    let client = EasClient::new(boundary);
    let result =
        tokio::time::timeout(Duration::from_millis(10), client.search(123, "query", 0, 10, 500))
            .await;
    anyhow::ensure!(result.is_err(), "scripted command should have timed out");
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn options_requires_every_command_used_by_the_client() -> anyhow::Result<()> {
    let headers = BTreeMap::from([
        ("ms-asprotocolversions".into(), "14.1".into()),
        ("ms-asprotocolcommands".into(), "Provision,FolderSync,Sync".into()),
    ]);
    let transport =
        Arc::new(ScriptedTransport::new(vec![ExpectedCall::Options { status: 200, headers }]));
    let boundary: Arc<dyn Transport> = transport.clone();
    let result = EasClient::new(boundary).options().await;
    anyhow::ensure!(matches!(result, Err(EasError::Protocol(_))));
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn both_remote_wipe_variants_are_acknowledged_without_retry() -> anyhow::Result<()> {
    for account_only in [false, true] {
        let transport = Arc::new(ScriptedTransport::new(vec![
            ExpectedCall::Command {
                command: Command::Provision,
                body: build_initial_provision()?,
                policy_key: None,
                safety: RequestSafety::RetrySafe,
                status: 200,
                response: wipe_response(account_only)?,
                delay: Duration::ZERO,
                failure: None,
            },
            ExpectedCall::Command {
                command: Command::Provision,
                body: build_wipe_ack(account_only)?,
                policy_key: Some(77),
                safety: RequestSafety::Mutation,
                status: 200,
                response: Vec::new(),
                delay: Duration::ZERO,
                failure: None,
            },
        ]));
        let boundary: Arc<dyn Transport> = transport.clone();
        let result = EasClient::new(boundary).provision().await;
        anyhow::ensure!(matches!(result, Err(EasError::AccountRemoteWipe)));
        transport.verify_complete()?;
    }
    Ok(())
}

fn expected(command: Command, body: Vec<u8>, response: Vec<u8>) -> ExpectedCall {
    ExpectedCall::Command {
        command,
        body,
        policy_key: Some(123),
        safety: RequestSafety::RetrySafe,
        status: 200,
        response,
        delay: Duration::ZERO,
        failure: None,
    }
}

fn options_headers() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("ms-asprotocolversions".into(), "14.1".into()),
        (
            "ms-asprotocolcommands".into(),
            "Provision,FolderSync,Sync,Search,ItemOperations,SendMail,SmartReply,SmartForward"
                .into(),
        ),
    ])
}

fn account() -> AccountConfig {
    AccountConfig {
        profile: ProfileKey::default(),
        email: "user@example.invalid".into(),
        username: "example_user".into(),
        enabled: true,
        write_enabled: false,
    }
}

fn secret_store() -> MemorySecretStore {
    let mut bundle = SecretBundle::new();
    bundle.accounts.insert(
        "example".into(),
        AccountSecret {
            password: "fixture-value".into(),
            device_id: "0011223344556677".into(),
            policy_key: 123,
            policy: Some(eas_mail_mcp::StoredPolicy {
                max_attachment_bytes: 25 * 1024 * 1024,
                attachments_enabled: true,
                body_limit: 50_000,
                mail_filter_type: 5,
                calendar_filter_type: 6,
            }),
        },
    );
    MemorySecretStore::with_bundle(bundle)
}

fn folder_response() -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("FolderHierarchy", "FolderSync");
    root.push(Element::text("FolderHierarchy", "Status", "1"));
    root.push(Element::text("FolderHierarchy", "SyncKey", "1"));
    let mut changes = Element::new("FolderHierarchy", "Changes");
    let mut add = Element::new("FolderHierarchy", "Add");
    add.push(Element::text("FolderHierarchy", "ServerId", "inbox"));
    add.push(Element::text("FolderHierarchy", "ParentId", "0"));
    add.push(Element::text("FolderHierarchy", "DisplayName", "Inbox"));
    add.push(Element::text("FolderHierarchy", "Type", "2"));
    changes.push(add);
    root.push(changes);
    encode(&root)
}

fn sync_response(
    key: &str,
    more_available: bool,
    add_message: bool,
) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("AirSync", "Sync");
    let mut collections = Element::new("AirSync", "Collections");
    let mut collection = Element::new("AirSync", "Collection");
    collection.push(Element::text("AirSync", "SyncKey", key));
    collection.push(Element::text("AirSync", "Status", "1"));
    if more_available {
        collection.push(Element::new("AirSync", "MoreAvailable"));
    }
    if add_message {
        let mut commands = Element::new("AirSync", "Commands");
        let mut add = Element::new("AirSync", "Add");
        add.push(Element::text("AirSync", "ServerId", "message-1"));
        let mut data = Element::new("AirSync", "ApplicationData");
        data.push(Element::text("Email", "Subject", "Message"));
        add.push(data);
        commands.push(add);
        collection.push(commands);
    }
    collections.push(collection);
    root.push(collections);
    encode(&root)
}

fn wipe_response(account_only: bool) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("Provision", "Provision");
    root.push(Element::text("Provision", "Status", "1"));
    let mut policies = Element::new("Provision", "Policies");
    let mut policy = Element::new("Provision", "Policy");
    policy.push(Element::text("Provision", "PolicyKey", "77"));
    policies.push(policy);
    root.push(policies);
    let wipe = if account_only { "AccountOnlyRemoteWipe" } else { "RemoteWipe" };
    root.push(Element::new("Provision", wipe));
    encode(&root)
}
