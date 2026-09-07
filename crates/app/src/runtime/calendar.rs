use std::sync::Arc;

use futures::future::join_all;

use super::Runtime;
use super::calendar_agenda;
use super::convert::{calendar_event, calendar_event_summary};
use super::schedule::{self, AvailabilityPage, PreparedAvailability, SchedulePlan};
use crate::backend::{AccountBackend, BackendEvent};
use crate::model::{
    CalendarAvailabilityData, CalendarAvailabilityInput, CalendarEvent, CalendarFindSlotsInput,
    CalendarGetInput, CalendarSearchData, CalendarSearchInput, CalendarSlotsData,
};
use crate::sanitize::limit;
use crate::{ApiResponse, AppError, ErrorCode, Result};

impl Runtime {
    /// Resolves directory recipients and returns compact 30-minute free/busy intervals.
    pub async fn calendar_availability(
        &self,
        input: CalendarAvailabilityInput,
    ) -> ApiResponse<CalendarAvailabilityData> {
        Self::response(self.calendar_availability_result(input).await)
    }

    /// Finds chronological common windows entirely inside explicit working hours.
    pub async fn calendar_find_slots(
        &self,
        input: CalendarFindSlotsInput,
    ) -> ApiResponse<CalendarSlotsData> {
        Self::response(self.ranked_slots_result(input).await)
    }

    /// Searches own-calendar text or returns a compact bounded agenda.
    pub async fn calendar_search(
        &self,
        input: CalendarSearchInput,
    ) -> ApiResponse<CalendarSearchData> {
        Self::response(self.calendar_search_result(input).await)
    }

    /// Fetches one own-calendar event through ItemOperations.
    pub async fn calendar_get(&self, input: CalendarGetInput) -> ApiResponse<CalendarEvent> {
        Self::response(self.calendar_get_result(input).await)
    }

    async fn calendar_availability_result(
        &self,
        input: CalendarAvailabilityInput,
    ) -> Result<(CalendarAvailabilityData, Vec<crate::Warning>)> {
        let plan = schedule::plan(
            &input.participants,
            &input.date_from,
            &input.date_to,
            &input.time_zone,
            &input.working_hours,
        )?;
        let backend = self.calendar_backend(input.account_id.as_deref(), &input.participants)?;
        let prepared = self.request_availability(backend, &input.participants, &plan).await?;
        Ok((prepared.data, Vec::new()))
    }

    async fn request_availability(
        &self,
        backend: Arc<dyn AccountBackend>,
        participants: &[String],
        plan: &SchedulePlan,
    ) -> Result<PreparedAvailability> {
        let account_id = backend.account().account_id;
        let mut pages = Vec::with_capacity(plan.chunks.len());
        for range in &plan.chunks {
            let result = backend.calendar_availability(participants, range.start, range.end).await;
            let values = self.account_result(&account_id, result)?;
            pages.push(AvailabilityPage { range: *range, participants: values });
        }
        schedule::prepare(account_id, participants, plan, pages)
    }

    async fn calendar_search_result(
        &self,
        input: CalendarSearchInput,
    ) -> Result<(CalendarSearchData, Vec<crate::Warning>)> {
        let plan = calendar_agenda::plan(&input)?;
        let result_limit = limit(input.limit.map(u32::from), 50, 100)?;
        let backends = self.selected(input.account_ids.as_deref())?;
        let results = join_all(backends.into_iter().map(|backend| {
            let plan = plan.clone();
            async move {
                let account_id = backend.account().account_id;
                let result = if plan.uses_agenda_scan() {
                    backend.scan_calendar_metadata().await.and_then(|mut result| {
                        result.events = plan.apply(result.events)?;
                        result.total = result.events.len();
                        Ok(result)
                    })
                } else {
                    backend.search_calendar(plan.query().unwrap_or_default(), result_limit).await
                };
                (account_id, result)
            }
        }))
        .await;
        let (groups, warnings) = self.collect_partial(results)?;
        let total = groups.iter().map(|group| group.total.max(group.events.len())).sum::<usize>();
        let mut events = groups.into_iter().flat_map(|group| group.events).collect::<Vec<_>>();
        events.sort_by_key(event_start);
        events.truncate(result_limit);
        let mut items = Vec::with_capacity(events.len());
        for event in events {
            let event_ref = self.references.insert_event(event.clone())?;
            items.push(calendar_event_summary(event_ref, &event));
        }
        let results_truncated = total > items.len();
        Ok((CalendarSearchData { items, results_truncated }, warnings))
    }

    async fn calendar_get_result(
        &self,
        input: CalendarGetInput,
    ) -> Result<(CalendarEvent, Vec<crate::Warning>)> {
        let body_limit = limit(input.body_limit, 12_000, 50_000)?;
        let reference = self.references.event(&input.event_ref)?;
        let backend = self.backend(&reference.account_id)?;
        let account_email = backend.account().email;
        let result = backend.fetch_calendar(&reference, body_limit).await;
        let event = self.account_result(&reference.account_id, result)?;
        let event = super::calendar_series::response::for_read(
            event,
            reference.occurrence_start,
            self.clock.now(),
        )?;
        Ok((calendar_event(input.event_ref, &event, &account_email, body_limit), Vec::new()))
    }

    pub(super) fn calendar_backend(
        &self,
        account_id: Option<&str>,
        participants: &[String],
    ) -> Result<Arc<dyn AccountBackend>> {
        if let Some(account_id) = account_id {
            return self.backend(account_id);
        }
        if self.backends.len() == 1 {
            return self.backends.values().next().cloned().ok_or_else(selection_error);
        }
        let domains = participants
            .iter()
            .map(|value| participant_domain(value).ok_or_else(selection_error))
            .collect::<Result<Vec<_>>>()?;
        let matches = self
            .backends
            .values()
            .filter(|backend| {
                let account = backend.account();
                domains.iter().all(|domain| {
                    account.email_domains.iter().any(|value| value.eq_ignore_ascii_case(domain))
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            return matches.into_iter().next().ok_or_else(selection_error);
        }
        Err(selection_error())
    }
}

fn participant_domain(value: &str) -> Option<&str> {
    let (_, domain) = value.trim().rsplit_once('@')?;
    (!domain.is_empty()).then_some(domain)
}

fn event_start(event: &BackendEvent) -> Option<chrono::DateTime<chrono::Utc>> {
    match event.fields.starts_at {
        eas_mail_protocol::Patch::Value(value) => value,
        eas_mail_protocol::Patch::Missing => None,
    }
}

fn selection_error() -> AppError {
    AppError::new(
        ErrorCode::AccountSelectionRequired,
        "select one account_id because participant domains do not identify a unique account",
    )
}
