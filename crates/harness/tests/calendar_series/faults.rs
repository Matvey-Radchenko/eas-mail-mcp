use super::series_support::{agenda, create, data, runtime, uuid};
use anyhow::Context as _;
use eas_mail_mcp::{
    CalendarAttendeeInput, CalendarAttendeeRole, CalendarOperationState, CalendarUpdateInput,
    ErrorCode,
};
use eas_mail_mcp_harness::FakeBackend;
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn every_split_checkpoint_stops_and_replays_without_resending() -> anyhow::Result<()> {
    for (operation, after, expected_steps) in [
        ("calendar_create_item", 0, vec![]),
        ("calendar_update_item", 0, vec!["new_series"]),
        ("calendar_send", 0, vec!["new_series", "truncate_old_series"]),
        ("calendar_send", 1, vec!["notify_current_attendees", "new_series", "truncate_old_series"]),
    ] {
        for code in [ErrorCode::ProtocolError, ErrorCode::OutcomeUnknown] {
            let backend = Arc::new(FakeBackend::new("work"));
            let (runtime, _directory) = runtime(backend.clone())?;
            let mut create = create(40)?;
            create.attendees.push(CalendarAttendeeInput {
                email: "guest@example.invalid".into(),
                name: None,
                role: CalendarAttendeeRole::Required,
            });
            data(runtime.calendar_create(create).await)?;
            let reference = agenda(&runtime).await?.get(2).context("third")?.event_ref.clone();
            backend.fail_calendar_step_after(operation, after, code)?;
            let input: CalendarUpdateInput = serde_json::from_value(json!({
                "event_ref":reference,"scope":"following","subject":"Tail","idempotency_key":uuid(41)
            }))?;
            let first = data(runtime.calendar_update(input.clone()).await)?;
            assert_eq!(first.completed_steps, expected_steps);
            let state = if code == ErrorCode::OutcomeUnknown {
                CalendarOperationState::Unknown
            } else if expected_steps.is_empty() {
                CalendarOperationState::Failed
            } else {
                CalendarOperationState::Partial
            };
            assert_eq!(first.status, state);
            let calls = backend.operations()?;
            let repeated = data(runtime.calendar_update(input.clone()).await)?;
            assert_eq!(repeated.status, state);
            assert_eq!(backend.operations()?, calls);
            let mut conflicting = input;
            conflicting.subject = Some("Different".into());
            let conflict = runtime.calendar_update(conflicting).await;
            assert_eq!(conflict.error.map(|e| e.code), Some(ErrorCode::IdempotencyConflict));
            assert_eq!(backend.operations()?, calls);
        }
    }
    Ok(())
}
