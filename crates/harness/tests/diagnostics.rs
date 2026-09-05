use std::sync::Arc;

use eas_mail_mcp::backend::AccountBackend;
use eas_mail_mcp::{AccountHealthStatus, AccountSelection, ErrorCode, Runtime, SystemClock};
use eas_mail_mcp_harness::{FakeBackend, MemoryJournal, SequenceIds};

#[tokio::test]
async fn account_health_preserves_each_failure_and_refreshes_live_state() -> anyhow::Result<()> {
    let healthy = Arc::new(FakeBackend::new("healthy"));
    let offline = Arc::new(FakeBackend::failing("offline"));
    let (runtime, _directory) = runtime(vec![healthy.clone(), offline.clone()])?;
    let response = runtime.accounts_status(AccountSelection::default()).await;
    assert!(response.error.is_none());
    let data = response.data.ok_or_else(|| anyhow::anyhow!("missing account health"))?;
    assert_eq!(data.accounts.len(), 2);
    assert!(data.accounts.iter().any(|account| account.account_id == "healthy"
        && account.status == AccountHealthStatus::Ready
        && account.capabilities.is_some()));
    assert!(data.accounts.iter().any(|account| account.account_id == "offline"
        && account.status == AccountHealthStatus::Failed
        && account.error_code == Some(ErrorCode::NetworkUnreachable)));
    healthy.set_failure(Some(ErrorCode::AuthRequired))?;
    let all_failed = runtime.accounts_status(AccountSelection::default()).await;
    assert!(all_failed.error.is_none());
    assert!(all_failed.data.is_some_and(|data| {
        data.accounts.iter().all(|account| account.status == AccountHealthStatus::Failed)
    }));
    offline.set_failure(None)?;
    let recovered = runtime
        .accounts_status(AccountSelection { account_ids: Some(vec!["offline".into()]) })
        .await;
    assert!(recovered.data.is_some_and(|data| {
        data.accounts.first().is_some_and(|account| account.status == AccountHealthStatus::Ready)
    }));
    assert!(healthy.operations()?.is_empty() && offline.operations()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn account_health_reports_remote_wipe_without_losing_other_accounts() -> anyhow::Result<()> {
    let wiped = Arc::new(FakeBackend::new("wiped"));
    wiped.set_failure(Some(ErrorCode::RemoteWipe))?;
    let (runtime, directory) = runtime(vec![wiped])?;
    let cached = directory.path().join("attachments/wiped");
    std::fs::create_dir_all(&cached)?;
    std::fs::write(cached.join("file.txt"), b"fixture")?;
    for _ in 0..2 {
        let response = runtime.accounts_status(AccountSelection::default()).await;
        assert!(response.error.is_none());
        assert!(response.data.is_some_and(|data| {
            data.accounts
                .first()
                .is_some_and(|account| account.error_code == Some(ErrorCode::RemoteWipe))
        }));
    }
    assert!(!cached.exists());
    Ok(())
}

#[tokio::test]
async fn account_health_validates_selection_and_does_not_expose_identity() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(vec![backend])?;
    for ids in [Vec::new(), vec!["missing".into()]] {
        let response = runtime.accounts_status(AccountSelection { account_ids: Some(ids) }).await;
        assert!(response.error.is_some_and(|error| error.code == ErrorCode::ValidationFailed));
    }
    let response = runtime.accounts_status(AccountSelection::default()).await;
    let value = serde_json::to_string(&response)?;
    assert!(!value.contains('@') && !value.contains("profile") && !value.contains("username"));
    Ok(())
}

fn runtime(backends: Vec<Arc<FakeBackend>>) -> anyhow::Result<(Runtime, tempfile::TempDir)> {
    let directory = tempfile::tempdir()?;
    let boundaries =
        backends.into_iter().map(|backend| -> Arc<dyn AccountBackend> { backend }).collect();
    let runtime = Runtime::with_dependencies(
        boundaries,
        Arc::new(MemoryJournal::default()),
        Arc::new(SystemClock),
        Arc::new(SequenceIds::default()),
        vec![7; 32],
        directory.path().join("attachments"),
    )?;
    Ok((runtime, directory))
}
