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

const TOOLS: [&str; 23] = [
    "accounts_list",
    "people_search",
    "folders_list",
    "sync_status",
    "sync_now",
    "mail_list",
    "mail_search",
    "mail_get",
    "mail_list_attachments",
    "mail_download_attachment",
    "calendar_availability",
    "calendar_find_slots",
    "calendar_search",
    "calendar_get",
    "mail_mark_read",
    "mail_send",
    "mail_reply",
    "mail_forward",
    "calendar_create",
    "calendar_update",
    "calendar_delete",
    "calendar_cancel",
    "calendar_respond",
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
    verify_tool_schemas(&tools)?;
    let invalid = call_result(&peer, "mail_get", Some(json!({}))).await?;
    anyhow::ensure!(invalid.is_error == Some(true), "invalid input did not fail schema validation");

    call(&peer, "accounts_list", None).await?;
    call(&peer, "people_search", Some(json!({"query":"Test", "limit":1}))).await?;
    call(&peer, "folders_list", Some(json!({}))).await?;
    call(&peer, "sync_status", Some(json!({}))).await?;
    call(&peer, "sync_now", Some(json!({}))).await?;

    let mail_page = call(&peer, "mail_list", Some(json!({ "limit": 1 }))).await?;
    let mail_ref = text_at(&mail_page, "/data/items/0/mail_ref")?;
    call(&peer, "mail_search", Some(json!({ "query": "quarterly" }))).await?;
    call(&peer, "mail_get", Some(json!({ "mail_ref": mail_ref, "body_limit": 12000 }))).await?;
    let attachment_page =
        call(&peer, "mail_list_attachments", Some(json!({ "mail_ref": mail_ref }))).await?;
    let attachment_ref = text_at(&attachment_page, "/data/attachments/0/attachment_ref")?;
    call(&peer, "mail_download_attachment", Some(json!({ "attachment_ref": attachment_ref })))
        .await?;

    exercise_calendar(&peer).await?;

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

fn verify_tool_schemas(tools: &[rmcp::model::Tool]) -> Result<()> {
    anyhow::ensure!(tools.iter().all(|tool| tool.output_schema.is_some()), "missing output schema");
    let schemas = serde_json::to_value(tools)?;
    anyhow::ensure!(
        !contains_numeric_format(&schemas),
        "tool schemas expose non-portable numeric formats"
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
    let sync_schema = tools
        .iter()
        .find(|tool| tool.name == "sync_now")
        .map(|tool| Value::Object(tool.input_schema.as_ref().clone()))
        .context("sync_now schema is missing")?;
    anyhow::ensure!(
        sync_schema.pointer("/properties/scope").is_none(),
        "sync_now still exposes the removed calendar scope"
    );
    let availability_schema = tools
        .iter()
        .find(|tool| tool.name == "calendar_availability")
        .map(|tool| Value::Object(tool.input_schema.as_ref().clone()))
        .context("calendar_availability schema is missing")?;
    anyhow::ensure!(
        availability_schema.pointer("/properties/participants/minItems").and_then(Value::as_u64)
            == Some(1)
            && availability_schema
                .pointer("/properties/participants/maxItems")
                .and_then(Value::as_u64)
                == Some(20),
        "calendar availability schema is missing participant bounds"
    );
    let slots_schema = tools
        .iter()
        .find(|tool| tool.name == "calendar_find_slots")
        .map(|tool| Value::Object(tool.input_schema.as_ref().clone()))
        .context("calendar_find_slots schema is missing")?;
    anyhow::ensure!(
        slots_schema.pointer("/properties/duration_minutes/minimum").and_then(Value::as_u64)
            == Some(15)
            && slots_schema.pointer("/properties/duration_minutes/maximum").and_then(Value::as_u64)
                == Some(480),
        "calendar slot schema is missing duration bounds"
    );
    let search_schema = tools
        .iter()
        .find(|tool| tool.name == "calendar_search")
        .map(|tool| Value::Object(tool.input_schema.as_ref().clone()))
        .context("calendar_search schema is missing")?;
    let required =
        search_schema.pointer("/required").and_then(Value::as_array).cloned().unwrap_or_default();
    anyhow::ensure!(
        !required.iter().any(|value| value == "query")
            && search_schema.pointer("/properties/date_from").is_some()
            && search_schema.pointer("/properties/date_to").is_some()
            && search_schema.pointer("/properties/time_zone").is_some()
            && search_schema.pointer("/properties/limit/maximum").and_then(Value::as_u64)
                == Some(100),
        "calendar_search schema is missing compact agenda inputs"
    );
    let create_schema = tools
        .iter()
        .find(|tool| tool.name == "calendar_create")
        .map(|tool| Value::Object(tool.input_schema.as_ref().clone()))
        .context("calendar_create schema is missing")?;
    anyhow::ensure!(
        create_schema.pointer("/properties/body/maxLength").and_then(Value::as_u64) == Some(50_000)
            && create_schema.pointer("/properties/attendees/maxItems").and_then(Value::as_u64)
                == Some(100),
        "calendar_create schema is missing write bounds"
    );
    Ok(())
}

async fn exercise_calendar(peer: &Peer<RoleClient>) -> Result<()> {
    let availability = json!({
        "participants": ["example@example.invalid"],
        "date_from": "2026-08-03",
        "date_to": "2026-08-03",
        "time_zone": "UTC",
        "working_hours": [{ "weekdays": ["mon"], "start": "09:00", "end": "18:00" }]
    });
    call(peer, "calendar_availability", Some(availability)).await?;
    call(
        peer,
        "calendar_find_slots",
        Some(json!({
            "participants": ["example@example.invalid"],
            "date_from": "2026-08-03",
            "date_to": "2026-08-03",
            "time_zone": "UTC",
            "working_hours": [{ "weekdays": ["mon"], "start": "09:00", "end": "18:00" }],
            "duration_minutes": 60
        })),
    )
    .await?;
    let page = call(peer, "calendar_search", Some(json!({ "query": "planning" }))).await?;
    let event_ref = text_at(&page, "/data/items/0/event_ref")?;
    call(peer, "calendar_get", Some(json!({ "event_ref": event_ref }))).await?;
    let agenda = call(
        peer,
        "calendar_search",
        Some(json!({
            "date_from": "2023-11-15",
            "date_to": "2023-11-15",
            "time_zone": "UTC"
        })),
    )
    .await?;
    let agenda_ref = text_at(&agenda, "/data/items/0/event_ref")?;
    call(peer, "calendar_get", Some(json!({ "event_ref": agenda_ref }))).await?;
    exercise_calendar_writes(peer).await?;
    Ok(())
}

async fn exercise_calendar_writes(peer: &Peer<RoleClient>) -> Result<()> {
    let meeting = call(
        peer,
        "calendar_create",
        Some(json!({
            "account_id": "example",
            "subject": "Harness meeting",
            "schedule": {
                "kind": "timed",
                "start": "2026-08-24T09:00:00Z",
                "end": "2026-08-24T10:00:00Z",
                "time_zone": "UTC"
            },
            "attendees": [{
                "email": "guest@example.invalid",
                "role": "required"
            }],
            "idempotency_key": "00000000-0000-4000-8000-000000000101"
        })),
    )
    .await?;
    let meeting_ref = text_at(&meeting, "/data/event_ref")?;
    let updated = call(
        peer,
        "calendar_update",
        Some(json!({
            "event_ref": meeting_ref,
            "subject": "Updated harness meeting",
            "idempotency_key": "00000000-0000-4000-8000-000000000102"
        })),
    )
    .await?;
    let updated_ref = text_at(&updated, "/data/event_ref")?;
    call(
        peer,
        "calendar_cancel",
        Some(json!({
            "event_ref": updated_ref,
            "comment": "Harness cleanup",
            "idempotency_key": "00000000-0000-4000-8000-000000000103"
        })),
    )
    .await?;
    let personal = call(
        peer,
        "calendar_create",
        Some(json!({
            "account_id": "example",
            "subject": "Harness personal event",
            "schedule": {
                "kind": "all_day",
                "start_date": "2026-08-25",
                "end_date": "2026-08-26",
                "time_zone": "UTC"
            },
            "idempotency_key": "00000000-0000-4000-8000-000000000104"
        })),
    )
    .await?;
    let personal_ref = text_at(&personal, "/data/event_ref")?;
    call(
        peer,
        "calendar_delete",
        Some(json!({
            "event_ref": personal_ref,
            "idempotency_key": "00000000-0000-4000-8000-000000000105"
        })),
    )
    .await?;
    let received = call(peer, "calendar_search", Some(json!({ "query": "received" }))).await?;
    let received_ref = text_at(&received, "/data/items/0/event_ref")?;
    call(
        peer,
        "calendar_respond",
        Some(json!({
            "event_ref": received_ref,
            "response": "accept",
            "comment": "Accepted by harness",
            "idempotency_key": "00000000-0000-4000-8000-000000000106"
        })),
    )
    .await?;
    Ok(())
}

fn contains_numeric_format(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(contains_numeric_format),
        Value::Object(fields) => {
            let numeric = fields.get("type").is_some_and(|value| match value {
                Value::Array(types) => {
                    types.iter().any(|value| matches!(value.as_str(), Some("integer" | "number")))
                }
                _ => matches!(value.as_str(), Some("integer" | "number")),
            });
            (numeric && fields.contains_key("format"))
                || fields.values().any(contains_numeric_format)
        }
        _ => false,
    }
}

#[tokio::test]
async fn black_box_independent_stdio_processes_start_cleanly() -> Result<()> {
    for _ in 0..24 {
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

#[tokio::test]
async fn object_references_cross_independent_stdio_processes() -> Result<()> {
    let first_transport =
        TokioChildProcess::new(tokio::process::Command::new(env!("CARGO_BIN_EXE_harness-server")))?;
    let first = ().serve(first_transport).await?;
    let first_peer = first.peer().clone();
    let mail_page = call(&first_peer, "mail_list", Some(json!({ "limit": 1 }))).await?;
    let mail_ref = text_at(&mail_page, "/data/items/0/mail_ref")?.to_owned();
    let attachments =
        call(&first_peer, "mail_list_attachments", Some(json!({ "mail_ref": mail_ref }))).await?;
    let attachment_ref = text_at(&attachments, "/data/attachments/0/attachment_ref")?.to_owned();
    let events = call(&first_peer, "calendar_search", Some(json!({ "query": "planning" }))).await?;
    let event_ref = text_at(&events, "/data/items/0/event_ref")?.to_owned();
    first.cancel().await?;

    let second_transport =
        TokioChildProcess::new(tokio::process::Command::new(env!("CARGO_BIN_EXE_harness-server")))?;
    let second = ().serve(second_transport).await?;
    let second_peer = second.peer().clone();
    call(&second_peer, "mail_get", Some(json!({ "mail_ref": mail_ref }))).await?;
    call(
        &second_peer,
        "mail_download_attachment",
        Some(json!({ "attachment_ref": attachment_ref })),
    )
    .await?;
    call(&second_peer, "calendar_get", Some(json!({ "event_ref": event_ref }))).await?;
    call(
        &second_peer,
        "mail_mark_read",
        Some(json!({
            "mail_ref": mail_ref,
            "is_read": true,
            "idempotency_key": "00000000-0000-4000-8000-000000000099"
        })),
    )
    .await?;
    second.cancel().await?;
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
