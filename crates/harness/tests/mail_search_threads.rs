use std::sync::Arc;

use eas_mail_mcp::backend::AccountBackend;
use eas_mail_mcp::{MailGetThreadInput, MailSearchFilters, MailSearchInput, Runtime};
use eas_mail_mcp_harness::{FakeBackend, FixedClock, MemoryJournal, SequenceIds};

#[tokio::test]
async fn search_hard_budget_and_coverage_survive_pagination() -> anyhow::Result<()> {
    let healthy = Arc::new(FakeBackend::new("work").with_mail_count(1001));
    let (runtime, _directory) = runtime(vec![healthy])?;
    let first = runtime
        .mail_search(MailSearchInput {
            query: "report".into(),
            limit: Some(1),
            ..Default::default()
        })
        .await;
    let first = first.data.ok_or_else(|| anyhow::anyhow!("no search data"))?;
    assert!(first.results_truncated);
    let coverage = first.coverage.first().ok_or_else(|| anyhow::anyhow!("no coverage"))?;
    assert_eq!(coverage.candidates_examined, 1000);
    assert_eq!(coverage.search_calls, 10);
    assert_eq!(coverage.estimated_total, Some(1001));
    let next = runtime
        .mail_search(MailSearchInput {
            cursor: first.next_cursor,
            limit: Some(1),
            ..Default::default()
        })
        .await;
    let next = next.data.ok_or_else(|| anyhow::anyhow!("no next page"))?;
    assert!(next.results_truncated);
    assert_eq!(next.coverage.first().map(|value| value.search_calls), Some(10));
    Ok(())
}

#[tokio::test]
async fn exact_filters_and_partial_source_warning_survive_cursor() -> anyhow::Result<()> {
    let healthy = Arc::new(FakeBackend::new("work").with_mail_count(3));
    let failing = Arc::new(FakeBackend::failing("offline"));
    let (runtime, _directory) = runtime(vec![healthy, failing])?;
    let input = MailSearchInput {
        query: "report".into(),
        limit: Some(1),
        filters: MailSearchFilters {
            from: Some("SENDER@example.invalid".into()),
            is_read: Some(false),
            has_attachments: Some(true),
            ..Default::default()
        },
        ..Default::default()
    };
    let first = runtime.mail_search(input).await;
    assert_eq!(first.warnings.len(), 1);
    let first = first.data.ok_or_else(|| anyhow::anyhow!("no search data"))?;
    assert!(!first.results_truncated);
    let next = runtime
        .mail_search(MailSearchInput { cursor: first.next_cursor, ..Default::default() })
        .await;
    assert_eq!(next.warnings.len(), 1);
    Ok(())
}

#[tokio::test]
async fn thread_ref_crosses_process_and_total_body_budget_is_enforced() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work").with_mail_count(3));
    let (first, _first_dir) = runtime(vec![backend.clone()])?;
    let page = first
        .mail_search(MailSearchInput { query: "report".into(), ..Default::default() })
        .await
        .data
        .ok_or_else(|| anyhow::anyhow!("no search data"))?;
    let mail_ref = page.items.first().ok_or_else(|| anyhow::anyhow!("no mail"))?.mail_ref.clone();
    drop(first);
    let (second, _second_dir) = runtime(vec![backend])?;
    let response = second
        .mail_get_thread(MailGetThreadInput {
            mail_ref,
            limit: Some(2),
            body_limit: Some(100),
            total_body_limit: Some(5),
        })
        .await;
    let data =
        response.data.ok_or_else(|| anyhow::anyhow!("thread failed: {:?}", response.error))?;
    assert_eq!(data.items.len(), 2);
    assert!(data.results_truncated && data.bodies_truncated);
    assert!(data.items.iter().map(|item| item.body.chars().count()).sum::<usize>() <= 5);
    Ok(())
}

#[test]
fn flat_search_schema_accepts_filters_but_rejects_typos() -> anyhow::Result<()> {
    let input: MailSearchInput = serde_json::from_value(serde_json::json!({
        "from": "sender@example.invalid",
        "received_after": "2026-09-01T00:00:00Z",
        "received_before": "2026-09-03T00:00:00Z"
    }))?;
    assert_eq!(input.query, "");
    assert!(input.filters.from.is_some());
    assert!(
        serde_json::from_value::<MailSearchInput>(serde_json::json!({
            "query": "report", "fram": "sender@example.invalid"
        }))
        .is_err()
    );
    Ok(())
}

fn runtime(backends: Vec<Arc<FakeBackend>>) -> anyhow::Result<(Runtime, tempfile::TempDir)> {
    let directory = tempfile::tempdir()?;
    let backends = backends.into_iter().map(|value| value as Arc<dyn AccountBackend>).collect();
    Ok((
        Runtime::with_dependencies(
            backends,
            Arc::new(MemoryJournal::default()),
            Arc::new(FixedClock::new(chrono::DateTime::UNIX_EPOCH)),
            Arc::new(SequenceIds::default()),
            vec![7; 32],
            directory.path().join("attachments"),
        )?,
        directory,
    ))
}
