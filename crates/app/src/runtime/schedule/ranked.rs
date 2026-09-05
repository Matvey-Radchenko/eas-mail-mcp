mod evaluate;
mod plan;
mod records;
mod recurring;

use chrono::{Duration, Timelike};

use plan::PersonPlan;
pub(crate) use plan::{RankedPlan, build};
use records::Record;
pub(crate) use records::{Page, prepare};
pub(crate) use recurring::find as find_recurring;

use super::UtcInterval;
use crate::model::{
    CalendarFindSlotsInput, CalendarParticipantRole, CalendarSlotsData, ParticipantResolutionState,
};
use crate::{AppError, ErrorCode, Result};

pub(crate) fn find(
    account_id: String,
    plan: &RankedPlan,
    records: &[Record],
    input: &CalendarFindSlotsInput,
) -> Result<CalendarSlotsData> {
    let limit = crate::sanitize::limit(input.limit.map(u32::from), 20, 50)?;
    let mut suggestions = candidate_ranges(plan, input.duration_minutes)
        .into_iter()
        .map(|range| (range.start, evaluate::suggestion(plan, records, range, input)))
        .filter(|(_, suggestion)| evaluate::required_safe(suggestion))
        .collect::<Vec<_>>();
    suggestions.sort_by_key(|(start, suggestion)| {
        let (issues, unknown) = evaluate::counts(suggestion, CalendarParticipantRole::Optional);
        (issues, unknown, suggestion.tentative_participants.len(), *start)
    });
    let mut windows = evaluate::windows(plan, records, input, limit + 1);
    let results_truncated = suggestions.len() > limit || windows.len() > limit;
    suggestions.truncate(limit);
    windows.truncate(limit);
    let data = CalendarSlotsData {
        account_id,
        time_zone: plan.schedule.time_zone.name().into(),
        duration_minutes: input.duration_minutes,
        precision_minutes: 30,
        resolution_complete: resolved(records),
        participants: records.iter().map(|record| record.summary.clone()).collect(),
        windows,
        suggestions: suggestions.into_iter().map(|(_, suggestion)| suggestion).collect(),
        buffer_minutes: input.buffer_minutes,
        results_truncated,
    };
    bounded(&data)?;
    Ok(data)
}

fn candidate_ranges(plan: &RankedPlan, duration_minutes: u16) -> Vec<UtcInterval> {
    let duration = Duration::minutes(i64::from(duration_minutes));
    let mut output = Vec::new();
    for work in &plan.schedule.working {
        let minute = work.start.with_timezone(&plan.schedule.time_zone).minute();
        let mut start = work.start + Duration::minutes(i64::from((15 - minute % 15) % 15));
        while start + duration <= work.end {
            output.push(UtcInterval { start, end: start + duration });
            start += Duration::minutes(15);
        }
    }
    output
}

fn resolved(records: &[Record]) -> bool {
    records.iter().all(|record| record.summary.resolution == ParticipantResolutionState::Resolved)
}

fn bounded(value: &impl serde::Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| {
        AppError::new(ErrorCode::ProtocolError, "calendar result cannot be encoded")
    })?;
    if bytes.len() > 256 * 1024 {
        return Err(AppError::new(
            ErrorCode::ResultTooLarge,
            "calendar suggestions exceed 256 KiB; reduce limit or participant count",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod boundary_tests;
#[cfg(test)]
mod tests;
