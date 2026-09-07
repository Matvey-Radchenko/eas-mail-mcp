use chrono::{DateTime, Days, Duration, NaiveDate, NaiveTime, Utc};
use eas_mail_mcp::{
    ApiResponse, CalendarAttendeeInput, CalendarAttendeeRole, CalendarCancelInput,
    CalendarDeleteInput, CalendarEvent, CalendarGetInput, CalendarOperationResult,
    CalendarOperationState, CalendarScheduleInput, ErrorCode, Runtime,
};

use super::super::checks::required;
use super::super::write_outcome::{incomplete, must_stop};
use super::LiveAccount;
use super::lookup::{self, ExpectedEvent};

#[derive(Debug, Clone)]
pub struct TimedSchedule {
    pub input: CalendarScheduleInput,
}

pub async fn cleanup_owned_event(
    runtime: &Runtime,
    account_id: &str,
    token: &str,
    current_ref: Option<&str>,
) -> anyhow::Result<()> {
    let mut event = None;
    if let Some(event_ref) = current_ref {
        event = runtime
            .calendar_get(CalendarGetInput {
                event_ref: event_ref.to_owned(),
                body_limit: Some(12_000),
            })
            .await
            .data;
    }
    if event.is_none() {
        event =
            lookup::find_event(runtime, account_id, token, None, ExpectedEvent::Personal).await?;
    }
    if event.is_none() {
        event =
            lookup::find_event(runtime, account_id, token, None, ExpectedEvent::Organizer).await?;
    }
    let Some(event) = event else {
        return Ok(());
    };
    remove_event(runtime, event).await
}

async fn remove_event(runtime: &Runtime, event: CalendarEvent) -> anyhow::Result<()> {
    if event.can_cancel {
        removed(
            runtime
                .calendar_cancel(CalendarCancelInput {
                    scope: None,
                    event_ref: event.event_ref,
                    comment: "Release harness failure cleanup".into(),
                    idempotency_key: operation_id(),
                })
                .await,
            "calendar_cancel cleanup",
        )?;
    } else if event.can_delete {
        removed(
            runtime
                .calendar_delete(CalendarDeleteInput {
                    scope: None,
                    event_ref: event.event_ref,
                    idempotency_key: operation_id(),
                })
                .await,
            "calendar_delete cleanup",
        )?;
    } else {
        anyhow::bail!("test Calendar item cannot be cleaned by this account")
    }
    Ok(())
}

fn removed(response: ApiResponse<CalendarOperationResult>, operation: &str) -> anyhow::Result<()> {
    if response.error.as_ref().is_some_and(|error| error.code == ErrorCode::NotFound) {
        return Ok(());
    }
    succeeded(response, operation).map(|_| ())
}

pub async fn get_event(runtime: &Runtime, event_ref: &str) -> anyhow::Result<CalendarEvent> {
    required(
        runtime
            .calendar_get(CalendarGetInput {
                event_ref: event_ref.to_owned(),
                body_limit: Some(12_000),
            })
            .await,
        "calendar_get lifecycle item",
    )
}

pub fn succeeded(
    response: ApiResponse<CalendarOperationResult>,
    operation: &str,
) -> anyhow::Result<CalendarOperationResult> {
    let result = required(response, operation)?;
    if matches!(result.status, CalendarOperationState::Partial | CalendarOperationState::Unknown) {
        let state =
            if result.status == CalendarOperationState::Partial { "Partial" } else { "Unknown" };
        return Err(incomplete(operation, state, Some(&result.operation_id))
            .context(format!("confirmed Calendar steps: {:?}", result.completed_steps)));
    }
    anyhow::ensure!(
        result.status == CalendarOperationState::Succeeded,
        "{operation} returned {:?} after steps {:?}; operation_id={}",
        result.status,
        result.completed_steps,
        result.operation_id
    );
    Ok(result)
}

pub fn timed_schedule(day_offset: u64, hour: u32, minute: u32) -> anyhow::Result<TimedSchedule> {
    let date = future_date(day_offset)?;
    let time = NaiveTime::from_hms_opt(hour, minute, 0)
        .ok_or_else(|| anyhow::anyhow!("invalid harness time"))?;
    let starts_at = DateTime::<Utc>::from_naive_utc_and_offset(date.and_time(time), Utc);
    let ends_at = starts_at + Duration::minutes(45);
    Ok(TimedSchedule {
        input: CalendarScheduleInput::Timed {
            start: starts_at.to_rfc3339(),
            end: ends_at.to_rfc3339(),
            time_zone: "UTC".into(),
        },
    })
}

pub fn all_day_schedule(day_offset: u64) -> anyhow::Result<CalendarScheduleInput> {
    let start = future_date(day_offset)?;
    let end = start
        .checked_add_days(Days::new(1))
        .ok_or_else(|| anyhow::anyhow!("all-day harness date overflow"))?;
    Ok(CalendarScheduleInput::AllDay {
        start_date: start.to_string(),
        end_date: end.to_string(),
        time_zone: "UTC".into(),
    })
}

fn future_date(day_offset: u64) -> anyhow::Result<NaiveDate> {
    Utc::now()
        .date_naive()
        .checked_add_days(Days::new(day_offset))
        .ok_or_else(|| anyhow::anyhow!("harness date overflow"))
}

pub fn calendar_attendee(
    account: &LiveAccount,
    role: CalendarAttendeeRole,
) -> CalendarAttendeeInput {
    CalendarAttendeeInput { email: account.email.clone(), name: None, role }
}

pub fn required_ref(reference: &Option<String>) -> anyhow::Result<&str> {
    reference
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Calendar operation returned no event reference"))
}

pub fn operation_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn test_token() -> String {
    format!("mcp-{}", &uuid::Uuid::new_v4().simple().to_string()[..12])
}

pub fn combine_with_cleanup(
    outcome: anyhow::Result<()>,
    cleanup: anyhow::Result<()>,
) -> anyhow::Result<()> {
    match (outcome, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(cleanup_error)) if must_stop(&cleanup_error) => {
            Err(cleanup_error.context(format!("Earlier Calendar lifecycle failure: {error}")))
        }
        (Err(error), Err(cleanup_error)) => {
            Err(error.context(format!("Calendar cleanup also failed: {cleanup_error}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_cleanup_keeps_typed_marker_and_operation_identity() -> anyhow::Result<()> {
        let operation = "11111111-2222-4333-8444-555555555555";
        let cleanup = incomplete("calendar_delete", "Unknown", Some(operation));
        let error = combine_with_cleanup(Err(anyhow::anyhow!("earlier read failed")), Err(cleanup))
            .err()
            .ok_or_else(|| anyhow::anyhow!("expected failed cleanup"))?
            .context("outer lifecycle");
        assert!(must_stop(&error));
        assert!(format!("{error:#}").contains(operation));
        assert!(format!("{error:#}").contains("Unknown"));
        Ok(())
    }
}
