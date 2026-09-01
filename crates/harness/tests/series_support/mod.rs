use eas_mail_mcp::{
    ApiResponse, CalendarCreateInput, CalendarEventSummary, CalendarSearchInput, Runtime,
};
use eas_mail_mcp_harness::{FakeBackend, FixedClock, MemoryJournal, SequenceIds};
use serde_json::json;
use std::sync::Arc;

pub fn runtime(backend: Arc<FakeBackend>) -> anyhow::Result<(Runtime, tempfile::TempDir)> {
    let directory = tempfile::tempdir()?;
    let runtime = Runtime::with_dependencies(
        vec![backend],
        Arc::new(MemoryJournal::default()),
        Arc::new(FixedClock::new(chrono::DateTime::UNIX_EPOCH)),
        Arc::new(SequenceIds::default()),
        vec![7; 32],
        directory.path().join("attachments"),
    )?;
    Ok((runtime, directory))
}

pub fn uuid(index: u8) -> String {
    format!("11111111-2222-4333-8444-5555555555{index:02}")
}

pub fn create(index: u8) -> anyhow::Result<CalendarCreateInput> {
    Ok(serde_json::from_value(json!({
        "account_id":"work", "subject":"Series test", "idempotency_key":uuid(index),
        "schedule":{"kind":"timed", "start":"2026-08-24T10:00:00Z", "end":"2026-08-24T11:00:00Z", "time_zone":"UTC"},
        "recurrence":{"frequency":"daily","end":{"mode":"count","count":5}}
    }))?)
}

pub fn data<T>(response: ApiResponse<T>) -> anyhow::Result<T> {
    response.data.ok_or_else(|| anyhow::anyhow!("runtime error: {:?}", response.error))
}

pub async fn agenda(runtime: &Runtime) -> anyhow::Result<Vec<CalendarEventSummary>> {
    Ok(data(
        runtime
            .calendar_search(CalendarSearchInput {
                query: None,
                date_from: Some("2026-08-24".into()),
                date_to: Some("2026-08-31".into()),
                time_zone: Some("UTC".into()),
                account_ids: None,
                limit: Some(100),
            })
            .await,
    )?
    .items)
}
