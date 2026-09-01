use std::sync::Arc;

use eas_mail_mcp::backend::AccountBackend;
use eas_mail_mcp::{
    CalendarAttendeeInput, CalendarAttendeeRole, CalendarBusyStatus, CalendarCancelInput,
    CalendarCreateInput, CalendarOperationState, CalendarRespondInput, CalendarResponseChoice,
    CalendarScheduleInput, CalendarSearchInput, ErrorCode, OperationJournal, Runtime,
};
use eas_mail_mcp_harness::{FakeBackend, FixedClock, MemoryJournal, SequenceIds};

#[tokio::test]
async fn calendar_body_and_attendee_limits_are_enforced_before_network() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory, journal) = make_runtime(backend.clone())?;

    let mut body_at_limit = create_input(uuid(1));
    body_at_limit.body = "x".repeat(50_000);
    assert_eq!(
        runtime.calendar_create(body_at_limit).await.data.map(|value| value.status),
        Some(CalendarOperationState::Succeeded)
    );

    let mut body_over_limit = create_input(uuid(2));
    body_over_limit.body = "x".repeat(50_001);
    assert_eq!(
        runtime.calendar_create(body_over_limit).await.error.map(|value| value.code),
        Some(ErrorCode::ValidationFailed)
    );
    assert!(journal.lookup(&uuid(2))?.is_none());

    let mut attendees_at_limit = create_input(uuid(3));
    attendees_at_limit.attendees = attendees(100);
    assert_eq!(
        runtime.calendar_create(attendees_at_limit).await.data.map(|value| value.status),
        Some(CalendarOperationState::Succeeded)
    );

    let mut attendees_over_limit = create_input(uuid(4));
    attendees_over_limit.attendees = attendees(101);
    assert_eq!(
        runtime.calendar_create(attendees_over_limit).await.error.map(|value| value.code),
        Some(ErrorCode::ValidationFailed)
    );
    assert!(journal.lookup(&uuid(4))?.is_none());
    assert_eq!(backend.operations()?.len(), 3);
    Ok(())
}

#[tokio::test]
async fn calendar_comments_accept_fifty_thousand_characters_and_reject_more() -> anyhow::Result<()>
{
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory, journal) = make_runtime(backend.clone())?;

    let cancelled = runtime
        .calendar_cancel(CalendarCancelInput {
            scope: None,
            event_ref: event_ref(&runtime, "planning").await?,
            comment: "x".repeat(50_000),
            idempotency_key: uuid(10),
        })
        .await;
    assert_eq!(cancelled.data.map(|value| value.status), Some(CalendarOperationState::Succeeded));
    let responded = runtime
        .calendar_respond(CalendarRespondInput {
            scope: None,
            event_ref: event_ref(&runtime, "received").await?,
            response: CalendarResponseChoice::Tentative,
            comment: "x".repeat(50_000),
            idempotency_key: uuid(11),
        })
        .await;
    assert_eq!(responded.data.map(|value| value.status), Some(CalendarOperationState::Succeeded));

    for (operation, response) in [
        (
            uuid(12),
            runtime
                .calendar_cancel(CalendarCancelInput {
                    scope: None,
                    event_ref: "unused".into(),
                    comment: "x".repeat(50_001),
                    idempotency_key: uuid(12),
                })
                .await,
        ),
        (
            uuid(13),
            runtime
                .calendar_respond(CalendarRespondInput {
                    scope: None,
                    event_ref: "unused".into(),
                    response: CalendarResponseChoice::Decline,
                    comment: "x".repeat(50_001),
                    idempotency_key: uuid(13),
                })
                .await,
        ),
    ] {
        assert_eq!(response.error.map(|value| value.code), Some(ErrorCode::ValidationFailed));
        assert!(journal.lookup(&operation)?.is_none());
    }
    assert_eq!(backend.operations()?.len(), 4);
    Ok(())
}

fn make_runtime(
    backend: Arc<FakeBackend>,
) -> anyhow::Result<(Runtime, tempfile::TempDir, Arc<MemoryJournal>)> {
    let directory = tempfile::tempdir()?;
    let boundary: Arc<dyn AccountBackend> = backend;
    let journal = Arc::new(MemoryJournal::default());
    let journal_boundary: Arc<dyn OperationJournal> = journal.clone();
    let runtime = Runtime::with_dependencies(
        vec![boundary],
        journal_boundary,
        Arc::new(FixedClock::new(chrono::DateTime::UNIX_EPOCH)),
        Arc::new(SequenceIds::default()),
        vec![7; 32],
        directory.path().join("attachments"),
    )?;
    Ok((runtime, directory, journal))
}

async fn event_ref(runtime: &Runtime, query: &str) -> anyhow::Result<String> {
    runtime
        .calendar_search(CalendarSearchInput {
            query: Some(query.into()),
            date_from: None,
            date_to: None,
            time_zone: None,
            account_ids: Some(vec!["work".into()]),
            limit: Some(1),
        })
        .await
        .data
        .and_then(|result| result.items.into_iter().next())
        .map(|event| event.event_ref)
        .ok_or_else(|| anyhow::anyhow!("calendar search returned no event"))
}

fn create_input(idempotency_key: String) -> CalendarCreateInput {
    CalendarCreateInput {
        recurrence: None,
        account_id: "work".into(),
        subject: "Calendar boundary test".into(),
        schedule: CalendarScheduleInput::AllDay {
            start_date: "2026-08-24".into(),
            end_date: "2026-08-25".into(),
            time_zone: "UTC".into(),
        },
        body: String::new(),
        location: String::new(),
        reminder_minutes: None,
        busy_status: CalendarBusyStatus::Busy,
        attendees: Vec::new(),
        idempotency_key,
    }
}

fn attendees(count: usize) -> Vec<CalendarAttendeeInput> {
    (0..count)
        .map(|index| CalendarAttendeeInput {
            email: format!("guest{index}@example.invalid"),
            name: None,
            role: CalendarAttendeeRole::Required,
        })
        .collect()
}

fn uuid(index: u8) -> String {
    format!("22222222-3333-4444-8555-6666666666{index:02}")
}
