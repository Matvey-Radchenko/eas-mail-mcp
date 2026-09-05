use std::sync::Arc;

use super::Runtime;
use super::schedule::ranked::{self, Page, RankedPlan};
use crate::backend::AccountBackend;
use crate::model::{
    CalendarFindRecurringSlotsInput, CalendarFindSlotsInput, CalendarRecurringSlotsData,
    CalendarSlotsData,
};
use crate::{ApiResponse, Result, Warning};

impl Runtime {
    /// Ranks weekly wall-clock patterns, reporting conflicts for every occurrence.
    pub async fn calendar_find_recurring_slots(
        &self,
        input: CalendarFindRecurringSlotsInput,
    ) -> ApiResponse<CalendarRecurringSlotsData> {
        Self::response(self.recurring_slots_result(input).await)
    }

    pub(super) async fn ranked_slots_result(
        &self,
        input: CalendarFindSlotsInput,
    ) -> Result<(CalendarSlotsData, Vec<Warning>)> {
        let plan = ranked::build(&input, None)?;
        let backend = self.calendar_backend(input.account_id.as_deref(), &input.participants)?;
        let account_id = backend.account().account_id;
        let (pages, warnings) = self.slot_pages(backend, &input.participants, &plan).await?;
        let records = ranked::prepare(&input.participants, pages)?;
        Ok((ranked::find(account_id, &plan, &records, &input)?, warnings))
    }

    async fn recurring_slots_result(
        &self,
        input: CalendarFindRecurringSlotsInput,
    ) -> Result<(CalendarRecurringSlotsData, Vec<Warning>)> {
        crate::sanitize::limit(input.schedule.limit.map(u32::from), 5, 10)?;
        let plan = ranked::build(&input.schedule, Some(input.weekday))?;
        let backend = self
            .calendar_backend(input.schedule.account_id.as_deref(), &input.schedule.participants)?;
        let account_id = backend.account().account_id;
        let (pages, warnings) =
            self.slot_pages(backend, &input.schedule.participants, &plan).await?;
        let records = ranked::prepare(&input.schedule.participants, pages)?;
        Ok((ranked::find_recurring(account_id, &plan, &records, &input)?, warnings))
    }

    async fn slot_pages(
        &self,
        backend: Arc<dyn AccountBackend>,
        participants: &[String],
        plan: &RankedPlan,
    ) -> Result<(Vec<Page>, Vec<Warning>)> {
        let account_id = backend.account().account_id;
        let mut pages = Vec::new();
        let mut warnings = Vec::new();
        let mut last_failure = None;
        let mut throttled = false;
        for range in &plan.queries {
            if throttled {
                pages.push(Page { range: *range, participants: None });
                continue;
            }
            let result = backend.calendar_availability(participants, range.start, range.end).await;
            let values = match self.account_result(&account_id, result) {
                Ok(values) => Some(values),
                Err(error) if error.envelope.retryable => {
                    throttled = error.envelope.code == crate::ErrorCode::Throttled;
                    if !warnings
                        .iter()
                        .any(|warning: &Warning| warning.code == error.envelope.code.as_str())
                    {
                        warnings.push(Warning {
                            account_id: account_id.clone(), code: error.envelope.code.as_str().into(),
                            message: "Some calendar availability intervals are unknown because a read failed".into(),
                            retryable: true, remediation: error.envelope.remediation.clone(),
                            operation_id: error.envelope.operation_id.clone(), retry_after_seconds: error.envelope.retry_after_seconds,
                        });
                    }
                    last_failure = Some(error);
                    None
                }
                Err(error) => return Err(error),
            };
            pages.push(Page { range: *range, participants: values });
        }
        if !pages.iter().any(|page| page.participants.is_some())
            && let Some(error) = last_failure
        {
            return Err(error);
        }
        Ok((pages, warnings))
    }
}
