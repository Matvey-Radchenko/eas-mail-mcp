use anyhow::{Context as _, Result};
use chrono::{DateTime, Duration, Utc};
use eas_mail_mcp::{
    ApiResponse, CalendarBusyStatus, CalendarCreateInput, CalendarDeleteInput, CalendarEvent,
    CalendarEventType, CalendarFrequency, CalendarGetInput, CalendarOperationResult,
    CalendarOperationState, CalendarRecurrenceEnd, CalendarRecurrenceInput, CalendarScheduleInput,
    CalendarScope, CalendarSearchInput, CalendarUpdateInput, Runtime,
};

#[path = "resume.rs"]
pub(super) mod resume;

struct Fixture {
    account: String,
    subject: String,
    uid: String,
    reference: String,
    starts_at: DateTime<Utc>,
}

pub async fn personal(runtime: &Runtime, account: &str, all_day: bool) -> Result<()> {
    let fixture = Fixture::create(runtime, account, all_day, false).await?;
    let mut patch = fixture.patch(&fixture.reference, None);
    patch.schedule = Some(schedule(fixture.starts_at + Duration::days(1), all_day));
    patch.body = Some(String::new());
    patch.location = Some(String::new());
    patch.busy_status = Some(CalendarBusyStatus::Free);
    patch.clear_reminder = true;
    let updated = succeeded(runtime.calendar_update(patch).await)?;
    let reference = updated.event_ref.context("update returned no reference")?;
    let event = fixture.owned(runtime, &reference).await?;
    anyhow::ensure!(
        event.all_day == all_day
            && event.body.is_empty()
            && event.location.is_empty()
            && event.busy_status == CalendarBusyStatus::Free,
        "personal patch did not round-trip"
    );
    let expected = (fixture.starts_at + Duration::days(1)).date_naive();
    let actual = DateTime::parse_from_rfc3339(event.starts_at.as_deref().context("no start")?)?;
    anyhow::ensure!(actual.date_naive() == expected, "personal schedule did not round-trip");
    fixture.delete(runtime, &reference, None).await?;
    // ItemOperations currently reports a missing server item as a protocol status, not NOT_FOUND.
    // Confirm removal from a fresh complete agenda instead of treating arbitrary read errors as absence.
    anyhow::ensure!(
        fixture.occurrences(runtime).await?.is_empty(),
        "deleted personal event remains in agenda"
    );
    Ok(())
}

pub async fn recurring(runtime: &Runtime, account: &str) -> Result<()> {
    let fixture = Fixture::create(runtime, account, false, true).await?;
    let original = fixture.occurrences(runtime).await?;
    anyhow::ensure!(original.len() == 3, "weekly fixture did not expand to three occurrences");
    super::report(serde_json::json!({"stage":"weekly_created_and_read","occurrences":3}))?;
    let second = original.get(1).context("missing second occurrence")?;
    let third = original.get(2).context("missing third occurrence")?;
    let before = fixture.owned(runtime, &second.event_ref).await?;
    let mut patch = fixture.patch(&second.event_ref, Some(CalendarScope::Occurrence));
    patch.location = Some("Updated synthetic occurrence".into());
    let updated = succeeded(runtime.calendar_update(patch).await)?;
    let event = fixture
        .owned(runtime, updated.event_ref.as_deref().context("no updated reference")?)
        .await?;
    anyhow::ensure!(
        event.location == "Updated synthetic occurrence" && event.body == before.body,
        "occurrence patch was lost or changed unrelated body"
    );
    let patched = fixture.occurrences(runtime).await?;
    anyhow::ensure!(
        patched.len() == 3
            && patched
                .iter()
                .filter(|event| event.location == "Updated synthetic occurrence")
                .count()
                == 1,
        "occurrence update changed sibling occurrences"
    );
    super::report(serde_json::json!({"stage":"weekly_occurrence_updated","unchanged_siblings":2}))?;
    let final_location = resume::update_existing(runtime, &fixture, &patched).await?;
    fixture.delete(runtime, &third.event_ref, Some(CalendarScope::Occurrence)).await?;
    let remaining = fixture.occurrences(runtime).await?;
    anyhow::ensure!(
        remaining.len() == 2
            && remaining.iter().all(|event| event.starts_at != third.starts_at)
            && remaining.iter().filter(|event| event.location == final_location).count() == 1,
        "occurrence deletion did not preserve the other two dates"
    );
    super::report(
        serde_json::json!({"stage":"weekly_occurrence_deleted","remaining_occurrences":2}),
    )?;
    fixture.delete(runtime, &fixture.reference, Some(CalendarScope::Series)).await?;
    anyhow::ensure!(
        fixture.occurrences(runtime).await?.is_empty(),
        "deleted series remains in agenda"
    );
    Ok(())
}

impl Fixture {
    async fn create(
        runtime: &Runtime,
        account: &str,
        all_day: bool,
        recurring: bool,
    ) -> Result<Self> {
        let day = (Utc::now() + Duration::days(21)).date_naive();
        let starts_at = day.and_hms_opt(10, 0, 0).context("invalid fixture time")?.and_utc();
        let subject = format!("EAS Mail MCP calendar self-test {}", operation_id());
        let recurrence = recurring.then_some(CalendarRecurrenceInput {
            frequency: CalendarFrequency::Weekly,
            interval: 1,
            weekdays: Vec::new(),
            day_of_month: None,
            week_of_month: None,
            month: None,
            end: CalendarRecurrenceEnd::Count { count: 3 },
        });
        let created = succeeded(
            runtime
                .calendar_create(CalendarCreateInput {
                    account_id: account.into(),
                    subject: subject.clone(),
                    recurrence,
                    schedule: schedule(starts_at, all_day),
                    body: "Dedicated fresh personal acceptance fixture".into(),
                    location: "Synthetic calendar acceptance".into(),
                    reminder_minutes: Some(10),
                    busy_status: CalendarBusyStatus::Busy,
                    attendees: Vec::new(),
                    idempotency_key: operation_id(),
                })
                .await,
        )?;
        let reference = created.event_ref.context("create returned no reference")?;
        let event = required(
            runtime
                .calendar_get(CalendarGetInput {
                    event_ref: reference.clone(),
                    body_limit: Some(1000),
                })
                .await,
        )?;
        let fixture =
            Self { account: account.into(), subject, uid: event.uid.clone(), reference, starts_at };
        fixture.check(&event)?;
        anyhow::ensure!(
            event.body == "Dedicated fresh personal acceptance fixture",
            "created fixture body did not round-trip"
        );
        Ok(fixture)
    }

    fn check(&self, event: &CalendarEvent) -> Result<()> {
        anyhow::ensure!(
            event.account_id == self.account
                && event.subject == self.subject
                && !self.uid.is_empty()
                && event.uid == self.uid
                && event.event_type == CalendarEventType::Personal
                && event.attendees.is_empty()
                && event.can_update
                && event.can_delete
                && !event.can_cancel,
            "fixture ownership guard rejected event; no subsequent write attempted"
        );
        Ok(())
    }

    async fn owned(&self, runtime: &Runtime, reference: &str) -> Result<CalendarEvent> {
        let event = required(
            runtime
                .calendar_get(CalendarGetInput {
                    event_ref: reference.into(),
                    body_limit: Some(1000),
                })
                .await,
        )?;
        self.check(&event)?;
        Ok(event)
    }

    fn patch(&self, reference: &str, scope: Option<CalendarScope>) -> CalendarUpdateInput {
        CalendarUpdateInput {
            event_ref: reference.into(),
            scope,
            recurrence: None,
            subject: None,
            schedule: None,
            body: None,
            location: None,
            reminder_minutes: None,
            clear_reminder: false,
            busy_status: None,
            attendees: None,
            idempotency_key: operation_id(),
        }
    }

    async fn delete(
        &self,
        runtime: &Runtime,
        reference: &str,
        scope: Option<CalendarScope>,
    ) -> Result<()> {
        self.owned(runtime, reference).await?;
        succeeded(
            runtime
                .calendar_delete(CalendarDeleteInput {
                    event_ref: reference.into(),
                    scope,
                    idempotency_key: operation_id(),
                })
                .await,
        )?;
        Ok(())
    }

    async fn occurrences(&self, runtime: &Runtime) -> Result<Vec<CalendarEvent>> {
        let page = required(
            runtime
                .calendar_search(CalendarSearchInput {
                    query: Some(self.subject.clone()),
                    date_from: Some(self.starts_at.date_naive().to_string()),
                    date_to: Some((self.starts_at + Duration::days(15)).date_naive().to_string()),
                    time_zone: Some("UTC".into()),
                    account_ids: Some(vec![self.account.clone()]),
                    limit: Some(10),
                })
                .await,
        )?;
        anyhow::ensure!(!page.results_truncated, "synthetic agenda was truncated");
        let mut events = Vec::new();
        for summary in page.items {
            anyhow::ensure!(
                summary.subject == self.subject && summary.account_id == self.account,
                "synthetic agenda returned unrelated content"
            );
            events.push(self.owned(runtime, &summary.event_ref).await?);
        }
        events.sort_by(|left, right| left.starts_at.cmp(&right.starts_at));
        Ok(events)
    }
}

fn schedule(start: DateTime<Utc>, all_day: bool) -> CalendarScheduleInput {
    if all_day {
        CalendarScheduleInput::AllDay {
            start_date: start.date_naive().to_string(),
            end_date: (start + Duration::days(1)).date_naive().to_string(),
            time_zone: "UTC".into(),
        }
    } else {
        CalendarScheduleInput::Timed {
            start: start.to_rfc3339(),
            end: (start + Duration::minutes(45)).to_rfc3339(),
            time_zone: "UTC".into(),
        }
    }
}

pub fn required<T>(response: ApiResponse<T>) -> Result<T> {
    if let Some(error) = response.error {
        anyhow::bail!("calendar acceptance failed: {:?}; no automatic write retry", error.code);
    }
    anyhow::ensure!(response.warnings.is_empty(), "calendar acceptance returned partial warnings");
    response.data.context("calendar acceptance returned no result")
}

fn succeeded(response: ApiResponse<CalendarOperationResult>) -> Result<CalendarOperationResult> {
    let result = required(response)?;
    anyhow::ensure!(
        result.status == CalendarOperationState::Succeeded,
        "calendar write not confirmed: {:?}; operation {}; stop without cleanup or retry",
        result.status,
        result.operation_id
    );
    Ok(result)
}

fn operation_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
