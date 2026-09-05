use std::io::{self, Write as _};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context as _, Result};
use chrono::{Days, Utc};
use clap::Parser;
use rmcp::ServiceExt as _;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, Implementation, InitializeRequestParams,
};
use rmcp::service::{Peer, RoleClient};
use rmcp::transport::{ConfigureCommandExt as _, TokioChildProcess};
use serde::Serialize;
use serde_json::{Value, json};

#[path = "live_harness/artifact_outcome.rs"]
mod artifact_outcome;
#[expect(dead_code, reason = "artifact responses use JSON rather than typed ApiResponse warnings")]
#[path = "live_harness/write_outcome.rs"]
mod write_outcome;

#[derive(Debug, Parser)]
struct Arguments {
    #[arg(long)]
    binary: PathBuf,
    #[arg(long)]
    self_write: bool,
    #[arg(long)]
    meeting_diagnostics: bool,
}

#[derive(Debug, Serialize)]
struct Report {
    tools: usize,
    accounts: usize,
    read_smoke: bool,
    personal_write_accounts: usize,
    meeting_diagnostics: Option<MeetingDiagnostics>,
}

#[derive(Debug, Serialize)]
struct MeetingDiagnostics {
    recent_matches: usize,
    distinct_accounts: usize,
    classified: usize,
    actionable: usize,
    request: usize,
    update: usize,
    cancellation: usize,
    response: usize,
    other: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let arguments = Arguments::parse();
    if arguments.self_write {
        confirm()?;
    }
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(&arguments.binary).configure(|command| {
            command.arg("serve");
            command.kill_on_drop(true);
        }),
    )?;
    let info = InitializeRequestParams::new(
        ClientCapabilities::default(),
        Implementation::new("artifact-live-smoke", env!("CARGO_PKG_VERSION")),
    );
    let client = tokio::time::timeout(Duration::from_secs(30), info.serve(transport))
        .await
        .context("artifact MCP initialize timed out")??;
    let peer = client.peer().clone();
    let tools = peer.list_all_tools().await?;
    let expected = eas_mail_mcp_harness::contract::expected_tool_names()?;
    let actual =
        tools.iter().map(|tool| tool.name.to_string()).collect::<std::collections::BTreeSet<_>>();
    anyhow::ensure!(actual == expected, "artifact exposed an unexpected tool contract");

    let accounts_response = call(&peer, "accounts_list", None).await?;
    let accounts = accounts_response
        .pointer("/data/accounts")
        .and_then(Value::as_array)
        .context("accounts_list returned no accounts")?;
    anyhow::ensure!(!accounts.is_empty(), "artifact has no configured accounts");
    call(&peer, "folders_list", Some(json!({}))).await?;
    call(&peer, "sync_status", Some(json!({}))).await?;
    call(&peer, "mail_list", Some(json!({ "limit": 1 }))).await?;
    call(&peer, "calendar_search", Some(json!({ "query": "EAS Mail MCP", "limit": 1 }))).await?;
    let today = Utc::now().date_naive();
    let agenda_to = today.checked_add_days(Days::new(6)).unwrap_or(today);
    call(
        &peer,
        "calendar_search",
        Some(json!({
            "date_from": today.to_string(),
            "date_to": agenda_to.to_string(),
            "time_zone": "UTC",
            "limit": 100
        })),
    )
    .await?;
    let meeting_diagnostics =
        if arguments.meeting_diagnostics { Some(meeting_diagnostics(&peer).await?) } else { None };

    let mut writes = 0;
    if arguments.self_write {
        for account in accounts {
            let account_id = account
                .get("account_id")
                .and_then(Value::as_str)
                .context("account has no identifier")?;
            personal_write(&peer, account_id).await?;
            writes += 1;
        }
    }
    client.cancel().await?;
    serde_json::to_writer_pretty(
        io::stdout(),
        &Report {
            tools: tools.len(),
            accounts: accounts.len(),
            read_smoke: true,
            personal_write_accounts: writes,
            meeting_diagnostics,
        },
    )?;
    writeln!(io::stdout())?;
    Ok(())
}

async fn meeting_diagnostics(peer: &Peer<RoleClient>) -> Result<MeetingDiagnostics> {
    let response =
        call(peer, "mail_search", Some(json!({ "query": "EAS Mail MCP meeting", "limit": 100 })))
            .await?;
    let items = response
        .pointer("/data/items")
        .and_then(Value::as_array)
        .context("mail_search returned no items")?;
    let cutoff = Utc::now() - chrono::Duration::minutes(30);
    let recent = items.iter().filter(|item| {
        item.get("received_at")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<chrono::DateTime<Utc>>().ok())
            .is_some_and(|value| value >= cutoff)
    });
    let recent = recent.collect::<Vec<_>>();
    let distinct_accounts = recent
        .iter()
        .filter_map(|item| item.get("account_id").and_then(Value::as_str))
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    Ok(MeetingDiagnostics {
        recent_matches: recent.len(),
        distinct_accounts,
        classified: recent
            .iter()
            .filter(|item| item.get("calendar_message").is_some_and(|value| !value.is_null()))
            .count(),
        actionable: recent
            .iter()
            .filter(|item| item.get("can_respond").and_then(Value::as_bool) == Some(true))
            .count(),
        request: calendar_kind(&recent, "request"),
        update: calendar_kind(&recent, "update"),
        cancellation: calendar_kind(&recent, "cancellation"),
        response: calendar_kind(&recent, "response"),
        other: calendar_kind(&recent, "other"),
    })
}

fn calendar_kind(items: &[&Value], expected: &str) -> usize {
    items
        .iter()
        .filter(|item| item.get("calendar_message").and_then(Value::as_str) == Some(expected))
        .count()
}

async fn personal_write(peer: &Peer<RoleClient>, account_id: &str) -> Result<()> {
    let start = Utc::now()
        .date_naive()
        .checked_add_days(Days::new(30))
        .context("artifact smoke date overflow")?;
    let end = start.checked_add_days(Days::new(1)).context("artifact smoke date overflow")?;
    let created = call(
        peer,
        "calendar_create",
        Some(json!({
            "account_id": account_id,
            "subject": "EAS Mail MCP artifact smoke",
            "schedule": {
                "kind": "all_day",
                "start_date": start.to_string(),
                "end_date": end.to_string(),
                "time_zone": "UTC"
            },
            "idempotency_key": uuid::Uuid::new_v4().to_string()
        })),
    )
    .await?;
    let event_ref = created
        .pointer("/data/event_ref")
        .and_then(Value::as_str)
        .context("calendar_create returned no event reference")?
        .to_owned();
    let checked = call(
        peer,
        "calendar_get",
        Some(json!({ "event_ref": event_ref.clone(), "body_limit": 12000 })),
    )
    .await;
    if checked.as_ref().is_err_and(write_outcome::must_stop) {
        return checked.map(|_| ());
    }
    let cleanup = call(
        peer,
        "calendar_delete",
        Some(json!({
            "event_ref": event_ref,
            "idempotency_key": uuid::Uuid::new_v4().to_string()
        })),
    )
    .await;
    if cleanup.as_ref().is_err_and(write_outcome::must_stop) {
        return cleanup.map(|_| ());
    }
    checked?;
    cleanup?;
    Ok(())
}

async fn call(peer: &Peer<RoleClient>, name: &str, input: Option<Value>) -> Result<Value> {
    let mut request = CallToolRequestParams::new(name.to_owned());
    if let Some(input) = input {
        let arguments = input.as_object().cloned().context("tool arguments must be an object")?;
        request = request.with_arguments(arguments);
    }
    let result = tokio::time::timeout(Duration::from_secs(60), peer.call_tool(request))
        .await
        .with_context(|| format!("{name} timed out"))??;
    let structured = result.structured_content.context("tool returned no structured content")?;
    artifact_outcome::validate(name, &structured)?;
    anyhow::ensure!(result.is_error != Some(true), "{name} returned an MCP tool error");
    Ok(structured)
}

fn confirm() -> Result<()> {
    writeln!(
        io::stderr(),
        "This creates and deletes one personal all-day event in every configured account."
    )?;
    write!(io::stderr(), "Type ARTIFACT-WRITE to continue: ")?;
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    anyhow::ensure!(
        input.trim() == "ARTIFACT-WRITE",
        "artifact write confirmation was not provided"
    );
    Ok(())
}
