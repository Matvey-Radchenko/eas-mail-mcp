use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use eas_mail_mcp::backend::AccountBackend;
use eas_mail_mcp::mcp::MailMcpServer;
use eas_mail_mcp::{ErrorCode, RandomIds, Runtime, SystemClock};
use eas_mail_mcp_harness::{FakeBackend, MemoryJournal};
use rmcp::ServiceExt as _;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::{RoleClient, RoleServer, RunningService};
use serde_json::{Value, json};

struct Session {
    client: RunningService<RoleClient, ()>,
    server: RunningService<RoleServer, MailMcpServer>,
    _directory: tempfile::TempDir,
}

impl Session {
    async fn start(backends: Vec<Arc<dyn AccountBackend>>) -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let runtime = Runtime::with_dependencies(
            backends,
            Arc::new(MemoryJournal::default()),
            Arc::new(SystemClock),
            Arc::new(RandomIds),
            vec![7; 32],
            directory.path().join("attachments"),
        )?;
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let server =
            tokio::spawn(
                async move { MailMcpServer::new(Arc::new(runtime)).serve(server_io).await },
            );
        let client = tokio::time::timeout(Duration::from_secs(10), ().serve(client_io)).await??;
        Ok(Self { client, server: server.await??, _directory: directory })
    }

    async fn call(&self, name: &str, input: Value) -> Result<CallToolResult> {
        let request = CallToolRequestParams::new(name.to_owned())
            .with_arguments(input.as_object().cloned().context("input must be an object")?);
        Ok(tokio::time::timeout(Duration::from_secs(10), self.client.call_tool(request)).await??)
    }

    async fn close(self) -> Result<()> {
        self.client.cancel().await?;
        self.server.waiting().await?;
        Ok(())
    }
}

fn envelope(response: &CallToolResult, is_error: bool) -> Result<Value> {
    assert_eq!(response.is_error, Some(is_error));
    let structured = response.structured_content.clone().context("missing structured response")?;
    for name in ["data", "error", "warnings"] {
        assert!(structured.get(name).is_some());
    }
    let wire = serde_json::to_value(response)?;
    let text = wire
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .context("missing JSON text fallback")?;
    assert_eq!(serde_json::from_str::<Value>(text)?, structured);
    Ok(structured)
}

fn send_input() -> Value {
    json!({"account_id":"work","to":["self@example.invalid"],"subject":"Synthetic contract","body":"fixture",
        "idempotency_key":"00000000-0000-4000-8000-000000000001"})
}

#[tokio::test]
async fn successful_and_partial_reads_keep_one_truthful_envelope() -> Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let session =
        Session::start(vec![backend.clone(), Arc::new(FakeBackend::failing("offline"))]).await?;
    let result = session.call("mail_list", json!({"limit":1})).await?;
    let listed = envelope(&result, false)?;
    assert!(listed.get("error").is_some_and(Value::is_null));
    assert_eq!(listed.pointer("/warnings/0/account_id"), Some(&json!("offline")));
    assert_eq!(listed.pointer("/data/items/0/untrusted_external_content"), Some(&json!(true)));
    let reference =
        listed.pointer("/data/items/0/mail_ref").cloned().context("missing reference")?;
    let detail = envelope(&session.call("mail_get", json!({"mail_ref":reference})).await?, false)?;
    assert_eq!(detail.pointer("/data/untrusted_external_content"), Some(&json!(true)));
    assert!(backend.operations()?.is_empty());
    let error = envelope(&session.call("mail_get", json!({"mail_ref":"invalid"})).await?, true)?;
    assert!(error.get("data").is_some_and(Value::is_null));
    assert!(error.pointer("/error/code").is_some());
    session.close().await
}

#[tokio::test]
async fn unknown_write_is_error_on_replay_but_operation_inspection_is_read_success() -> Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    backend.set_operation_failure(Some("mail_send"), ErrorCode::OutcomeUnknown)?;
    let session = Session::start(vec![backend.clone()]).await?;
    let initial = envelope(&session.call("mail_send", send_input()).await?, true)?;
    assert_eq!(initial.pointer("/error/code"), Some(&json!("OUTCOME_UNKNOWN")));
    assert_eq!(initial.pointer("/error/retryable"), Some(&json!(false)));
    backend.set_operation_failure(None, ErrorCode::OutcomeUnknown)?;
    let repeated = envelope(&session.call("mail_send", send_input()).await?, true)?;
    assert_eq!(repeated.pointer("/data/status"), Some(&json!("unknown")));
    let operation = envelope(
        &session
            .call(
                "operation_get",
                json!({
        "operation_id":"00000000-0000-4000-8000-000000000001"}),
            )
            .await?,
        false,
    )?;
    assert_eq!(operation.pointer("/data/status"), Some(&json!("unknown")));
    assert!(operation.pointer("/data/payload_hmac").is_none());
    assert!(backend.operations()?.is_empty());
    session.close().await
}

#[tokio::test]
async fn partial_calendar_write_has_machine_warning_and_never_replays_its_steps() -> Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    backend.set_operation_failure(Some("calendar_send"), ErrorCode::NetworkUnreachable)?;
    let session = Session::start(vec![backend.clone()]).await?;
    let input = json!({"account_id":"work","subject":"Synthetic meeting","schedule":{
        "kind":"timed","start":"2026-09-15T10:00:00Z","end":"2026-09-15T11:00:00Z","time_zone":"UTC"},
        "attendees":[{"email":"guest@example.invalid","role":"required"}],
        "idempotency_key":"00000000-0000-4000-8000-000000000002"});
    let partial = envelope(&session.call("calendar_create", input.clone()).await?, false)?;
    assert_eq!(partial.pointer("/data/status"), Some(&json!("partial")));
    assert_eq!(partial.pointer("/warnings/0/code"), Some(&json!("PARTIAL_WRITE")));
    assert_eq!(partial.pointer("/warnings/0/operation_id"), input.get("idempotency_key"));
    backend.set_operation_failure(None, ErrorCode::NetworkUnreachable)?;
    let replay = envelope(&session.call("calendar_create", input).await?, false)?;
    assert_eq!(replay.pointer("/data/status"), Some(&json!("partial")));
    assert_eq!(replay.pointer("/warnings/0/code"), Some(&json!("PARTIAL_WRITE")));
    assert_eq!(backend.operations()?, ["calendar_create_item"]);
    session.close().await
}

#[tokio::test]
async fn safe_failed_write_replay_is_not_reported_as_success() -> Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    backend.set_operation_failure(Some("mail_send"), ErrorCode::PolicyBlocked)?;
    let session = Session::start(vec![backend.clone()]).await?;
    envelope(&session.call("mail_send", send_input()).await?, true)?;
    backend.set_operation_failure(None, ErrorCode::PolicyBlocked)?;
    let repeated = envelope(&session.call("mail_send", send_input()).await?, true)?;
    assert_eq!(repeated.pointer("/data/status"), Some(&json!("failed")));
    assert!(backend.operations()?.is_empty());
    session.close().await
}

#[tokio::test]
async fn confirmed_single_write_and_replay_keep_success_without_duplicate_effects() -> Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let session = Session::start(vec![backend.clone()]).await?;
    for _ in 0..2 {
        let result = envelope(&session.call("mail_send", send_input()).await?, false)?;
        assert_eq!(result.pointer("/data/status"), Some(&json!("succeeded")));
        assert!(result.get("error").is_some_and(Value::is_null));
    }
    assert_eq!(backend.operations()?, ["mail_send"]);
    session.close().await
}
