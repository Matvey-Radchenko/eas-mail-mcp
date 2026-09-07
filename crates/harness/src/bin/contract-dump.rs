use std::io::{self, Write as _};
use std::sync::Arc;
use std::time::Duration;

use eas_mail_mcp::mcp::MailMcpServer;
use eas_mail_mcp::{RandomIds, Runtime, SystemClock};
use eas_mail_mcp_harness::{MemoryJournal, contract};
use rmcp::ServiceExt as _;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let runtime = Runtime::with_dependencies(
        Vec::new(),
        Arc::new(MemoryJournal::default()),
        Arc::new(SystemClock),
        Arc::new(RandomIds),
        vec![7; 32],
        directory.path().join("attachments"),
    )?;
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let server =
        tokio::spawn(async move { MailMcpServer::new(Arc::new(runtime)).serve(server_io).await });
    let client = tokio::time::timeout(Duration::from_secs(10), ().serve(client_io)).await??;
    let server = server.await??;
    let tools = client.list_all_tools().await?;
    let snapshot = serde_json::json!({
        "format_version":1, "release":"1.0", "mcp":contract::snapshot(&tools)?,
        "cli":eas_mail_mcp::cli::contract::snapshot(),
    });
    client.cancel().await?;
    server.waiting().await?;
    writeln!(io::stdout().lock(), "{}", serde_json::to_string_pretty(&snapshot)?)?;
    Ok(())
}
