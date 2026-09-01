use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use eas_mail_mcp::backend::AccountBackend;
use eas_mail_mcp::{Clock, IdGenerator, OperationJournal, Runtime, SqliteJournal};
use eas_mail_mcp_harness::{FakeBackend, FixedClock, SequenceIds};

const STATE_ENV: &str = "EAS_MAIL_HARNESS_STATE_DIR";

#[tokio::main]
async fn main() -> ExitCode {
    match runtime().map(Arc::new) {
        Ok(runtime) => finish(eas_mail_mcp::cli::run_with_runtime(runtime).await),
        Err(error) => finish(Err(error)),
    }
}

fn runtime() -> eas_mail_mcp::Result<Runtime> {
    let state = std::env::var_os(STATE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("eas-mail-mcp-harness-cli"));
    std::fs::create_dir_all(&state).map_err(|_| {
        eas_mail_mcp::AppError::new(
            eas_mail_mcp::ErrorCode::StorageError,
            "cannot create CLI harness state",
        )
    })?;
    let now = chrono::DateTime::from_timestamp(1_700_000_000, 0).ok_or_else(|| {
        eas_mail_mcp::AppError::new(
            eas_mail_mcp::ErrorCode::StorageError,
            "invalid deterministic time",
        )
    })?;
    let backends: Vec<Arc<dyn AccountBackend>> =
        vec![Arc::new(FakeBackend::new("example").with_mail_count(250).with_series_fixture())];
    let journal: Arc<dyn OperationJournal> = Arc::new(SqliteJournal::open(&state.join("journal"))?);
    let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(now));
    let ids: Arc<dyn IdGenerator> = Arc::new(SequenceIds::default());
    Runtime::with_dependencies(
        backends,
        journal,
        clock,
        ids,
        vec![7; 32],
        state.join("attachments"),
    )
}

fn finish(result: eas_mail_mcp::Result<eas_mail_mcp::cli::CliExit>) -> ExitCode {
    match result {
        Ok(status) => ExitCode::from(status.code()),
        Err(error) => {
            let payload = serde_json::to_string(&error.envelope)
                .unwrap_or_else(|_| "{\"code\":\"PROTOCOL_ERROR\"}".into());
            let _ = writeln!(std::io::stderr().lock(), "{payload}");
            let usage = matches!(
                error.envelope.code,
                eas_mail_mcp::ErrorCode::InteractiveRequired
                    | eas_mail_mcp::ErrorCode::ValidationFailed
            );
            let write_failed = error.envelope.operation_id.is_some()
                || error.envelope.code == eas_mail_mcp::ErrorCode::OutcomeUnknown;
            ExitCode::from(if write_failed {
                3
            } else if usage {
                2
            } else {
                1
            })
        }
    }
}
