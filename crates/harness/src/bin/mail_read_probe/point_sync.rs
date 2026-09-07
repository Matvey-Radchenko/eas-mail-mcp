use std::collections::BTreeMap;
use std::io::{self, Write as _};
use std::sync::Arc;

use eas_mail_protocol::wbxml::{Element, decode, encode};
use eas_mail_protocol::{Command, EasClient, HttpTransport, RequestSafety, Transport};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Locator {
    folder_id: String,
    server_id: String,
}

pub(super) async fn run(
    account: &eas_mail_mcp::AccountConfig,
    secret: &eas_mail_mcp::AccountSecret,
    profiles: &eas_mail_protocol::ProfileRegistry,
    path: &std::path::Path,
) -> anyhow::Result<()> {
    let locator: Locator = serde_json::from_slice(&std::fs::read(path)?)?;
    let transport = Arc::new(HttpTransport::new(
        profiles.require(&account.profile)?,
        account.username.clone(),
        secret.password.clone(),
        secret.device_id.clone(),
    )?);
    let client = EasClient::new(transport.clone());
    let before = client
        .fetch_item(secret.policy_key, None, Some(&locator.folder_id), Some(&locator.server_id), 1)
        .await;
    let initial = client
        .sync(
            secret.policy_key,
            &locator.folder_id,
            "0",
            eas_mail_protocol::CollectionKind::Mail,
            0,
            0,
        )
        .await?;
    anyhow::ensure!(initial.changes.is_empty(), "initial Sync unexpectedly returned items");
    let body = fetch_request(&locator, &initial.sync_key)?;
    let response = transport
        .command(Command::Sync, &body, Some(secret.policy_key), RequestSafety::RetrySafe)
        .await?;
    let tree = decode(&response.body)?;
    let mut tags = BTreeMap::new();
    if let Some(tree) = &tree {
        super::collect_shape(tree, "", &mut tags);
    }
    let report = super::WireReport {
        command: "InitialSyncThenPointFetch".into(),
        http_status: response.status,
        body_bytes: response.body.len(),
        decoded: tree.is_some(),
        tags,
    };
    serde_json::to_writer_pretty(
        io::stdout(),
        &serde_json::json!({
            "item_operations_before_succeeded": before.is_ok(), "wire": report,
        }),
    )?;
    writeln!(io::stdout())?;
    Ok(())
}

fn fetch_request(locator: &Locator, sync_key: &str) -> anyhow::Result<Vec<u8>> {
    let mut root = Element::new("AirSync", "Sync");
    let mut collections = Element::new("AirSync", "Collections");
    let mut collection = Element::new("AirSync", "Collection");
    collection.push(Element::text("AirSync", "SyncKey", sync_key));
    collection.push(Element::text("AirSync", "CollectionId", &locator.folder_id));
    collection.push(Element::text("AirSync", "GetChanges", "0"));
    let mut commands = Element::new("AirSync", "Commands");
    let mut fetch = Element::new("AirSync", "Fetch");
    fetch.push(Element::text("AirSync", "ServerId", &locator.server_id));
    commands.push(fetch);
    collection.push(commands);
    collections.push(collection);
    root.push(collections);
    Ok(encode(&root)?)
}
