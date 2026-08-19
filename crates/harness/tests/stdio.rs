use std::collections::BTreeSet;
use std::time::Duration;

use anyhow::{Context as _, Result};
use rmcp::ServiceExt as _;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, Implementation, InitializeRequestParams,
};
use rmcp::service::{Peer, RoleClient};
use rmcp::transport::{ConfigureCommandExt as _, TokioChildProcess};
use serde_json::{Value, json};

const TOOLS: [&str; 16] = [
    "accounts_list",
    "folders_list",
    "sync_status",
    "sync_now",
    "mail_list",
    "mail_search",
    "mail_get",
    "mail_list_attachments",
    "mail_download_attachment",
    "calendar_list",
    "calendar_search",
    "calendar_get",
    "mail_mark_read",
    "mail_send",
    "mail_reply",
    "mail_forward",
];

#[tokio::test]
async fn black_box_server_exposes_and_executes_every_tool() -> Result<()> {
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_harness-server")).configure(|command| {
            command.kill_on_drop(true);
        }),
    )?;
    let info = InitializeRequestParams::new(
        ClientCapabilities::builder().enable_elicitation().build(),
        Implementation::new("codex-mcp-client", "0.148.0-alpha.9"),
    );
    let client = tokio::time::timeout(Duration::from_secs(10), info.serve(transport))
        .await
        .context("MCP initialize timed out")??;
    let peer = client.peer().clone();
    let server = peer.peer_info().context("MCP server metadata is missing")?;
    let implementation = server.server_info.as_ref().context("MCP implementation is missing")?;
    anyhow::ensure!(implementation.version == env!("CARGO_PKG_VERSION"));

    let tools = peer.list_all_tools().await?;
    let names = tools.iter().map(|tool| tool.name.as_ref()).collect::<BTreeSet<_>>();
    let expected = TOOLS.into_iter().collect::<BTreeSet<_>>();
    anyhow::ensure!(names == expected, "unexpected tool contract: {names:?}");
    anyhow::ensure!(tools.iter().all(|tool| tool.output_schema.is_some()), "missing output schema");
    let schemas = serde_json::to_value(&tools)?;
    anyhow::ensure!(
        !contains_unsigned_format(&schemas),
        "tool schemas expose non-portable unsigned integer formats"
    );
    let send_schema = tools
        .iter()
        .find(|tool| tool.name == "mail_send")
        .map(|tool| Value::Object(tool.input_schema.as_ref().clone()))
        .context("mail_send schema is missing")?;
    anyhow::ensure!(
        send_schema.pointer("/properties/body/maxLength").and_then(Value::as_u64) == Some(50_000),
        "mail_send body schema is missing the 50,000 character limit"
    );
    let invalid = call_result(&peer, "mail_get", Some(json!({}))).await?;
    anyhow::ensure!(invalid.is_error == Some(true), "invalid input did not fail schema validation");

    call(&peer, "accounts_list", None).await?;
    call(&peer, "folders_list", Some(json!({}))).await?;
    call(&peer, "sync_status", Some(json!({}))).await?;
    call(&peer, "sync_now", Some(json!({ "scope": "all" }))).await?;

    let mail_page = call(&peer, "mail_list", Some(json!({ "limit": 1 }))).await?;
    let mail_ref = text_at(&mail_page, "/data/items/0/mail_ref")?;
    call(&peer, "mail_search", Some(json!({ "query": "quarterly" }))).await?;
    call(&peer, "mail_get", Some(json!({ "mail_ref": mail_ref, "body_limit": 12000 }))).await?;
    let attachment_page =
        call(&peer, "mail_list_attachments", Some(json!({ "mail_ref": mail_ref }))).await?;
    let attachment_ref = text_at(&attachment_page, "/data/attachments/0/attachment_ref")?;
    call(&peer, "mail_download_attachment", Some(json!({ "attachment_ref": attachment_ref })))
        .await?;

    let calendar_page = call(&peer, "calendar_list", Some(json!({ "limit": 1 }))).await?;
    let event_ref = text_at(&calendar_page, "/data/items/0/event_ref")?;
    call(&peer, "calendar_search", Some(json!({ "query": "planning" }))).await?;
    call(&peer, "calendar_get", Some(json!({ "event_ref": event_ref }))).await?;

    call(
        &peer,
        "mail_mark_read",
        Some(json!({
            "mail_ref": mail_ref,
            "is_read": true,
            "idempotency_key": "00000000-0000-4000-8000-000000000001"
        })),
    )
    .await?;
    call(
        &peer,
        "mail_send",
        Some(json!({
            "account_id": "example",
            "to": ["recipient@example.invalid"],
            "subject": "Harness send",
            "body": "body",
            "idempotency_key": "00000000-0000-4000-8000-000000000002"
        })),
    )
    .await?;
    call(
        &peer,
        "mail_reply",
        Some(json!({
            "mail_ref": mail_ref,
            "body": "reply",
            "idempotency_key": "00000000-0000-4000-8000-000000000003"
        })),
    )
    .await?;
    call(
        &peer,
        "mail_forward",
        Some(json!({
            "mail_ref": mail_ref,
            "to": ["recipient@example.invalid"],
            "idempotency_key": "00000000-0000-4000-8000-000000000004"
        })),
    )
    .await?;

    client.cancel().await?;
    Ok(())
}

fn contains_unsigned_format(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(contains_unsigned_format),
        Value::Object(fields) => {
            fields
                .get("format")
                .and_then(Value::as_str)
                .is_some_and(|value| matches!(value, "uint" | "uint64"))
                || fields.values().any(contains_unsigned_format)
        }
        _ => false,
    }
}

#[tokio::test]
async fn black_box_independent_stdio_processes_start_cleanly() -> Result<()> {
    for _ in 0..2 {
        let transport = TokioChildProcess::new(tokio::process::Command::new(env!(
            "CARGO_BIN_EXE_harness-server"
        )))?;
        let client = ().serve(transport).await?;
        let tools = client.list_all_tools().await?;
        anyhow::ensure!(tools.len() == TOOLS.len());
        client.cancel().await?;
    }
    Ok(())
}

async fn call(peer: &Peer<RoleClient>, name: &str, input: Option<Value>) -> Result<Value> {
    let result = call_result(peer, name, input).await?;
    let structured = result
        .structured_content
        .ok_or_else(|| anyhow::anyhow!("{name} returned no structured content"))?;
    anyhow::ensure!(
        structured.get("error").is_some_and(Value::is_null),
        "{name} failed: {structured}"
    );
    Ok(structured)
}

async fn call_result(
    peer: &Peer<RoleClient>,
    name: &str,
    input: Option<Value>,
) -> Result<rmcp::model::CallToolResult> {
    let mut request = CallToolRequestParams::new(name.to_owned());
    if let Some(input) = input {
        let arguments = input
            .as_object()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("tool arguments must be an object"))?;
        request = request.with_arguments(arguments);
    }
    let result = tokio::time::timeout(Duration::from_secs(10), peer.call_tool(request))
        .await
        .with_context(|| format!("{name} timed out"))??;
    Ok(result)
}

fn text_at<'a>(value: &'a Value, pointer: &str) -> Result<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing string at {pointer}"))
}
