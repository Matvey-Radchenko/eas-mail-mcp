use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use eas_mail_mcp::backend::AccountBackend;
use eas_mail_mcp::mcp::MailMcpServer;
use eas_mail_mcp::{Clock, IdGenerator, OperationJournal, Runtime};
use eas_mail_mcp_harness::{FakeBackend, FixedClock, MemoryJournal, SequenceIds};
use rmcp::ServiceExt as _;

const DELAY_ENV: &str = "EAS_MAIL_HARNESS_DELAY_MS";
const CLOCK_FILE_ENV: &str = "EAS_MAIL_HARNESS_CLOCK_FILE";

#[derive(Debug)]
struct FileClock {
    path: PathBuf,
    fallback: chrono::DateTime<chrono::Utc>,
}

impl Clock for FileClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|value| value.trim().parse::<i64>().ok())
            .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, 0))
            .unwrap_or(self.fallback)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let now = chrono::DateTime::from_timestamp(1_700_000_000, 0)
        .ok_or_else(|| anyhow::anyhow!("invalid harness time"))?;
    let delay = std::env::var(DELAY_ENV)
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()?
        .map_or(Duration::ZERO, Duration::from_millis);
    let backends: Vec<Arc<dyn AccountBackend>> =
        vec![Arc::new(FakeBackend::new("example").with_mail_count(2).with_delay(delay))];
    let journal: Arc<dyn OperationJournal> = Arc::new(MemoryJournal::default());
    let clock: Arc<dyn Clock> = std::env::var_os(CLOCK_FILE_ENV).map_or_else(
        || Arc::new(FixedClock::new(now)) as Arc<dyn Clock>,
        |path| Arc::new(FileClock { path: PathBuf::from(path), fallback: now }) as Arc<dyn Clock>,
    );
    let ids: Arc<dyn IdGenerator> = Arc::new(SequenceIds::default());
    let runtime = Arc::new(Runtime::with_dependencies(
        backends,
        journal,
        clock,
        ids,
        vec![7; 32],
        temporary.path().join("attachments"),
    )?);
    MailMcpServer::new(runtime).serve(rmcp::transport::stdio()).await?.waiting().await?;
    Ok(())
}
