use std::cmp::Reverse;
use std::collections::BTreeSet;

use chrono::{Duration, NaiveTime};

use super::super::{UtcInterval, time};
use super::{RankedPlan, Record, bounded, candidate_ranges, evaluate, resolved};
use crate::Result;
use crate::model::{
    CalendarFindRecurringSlotsInput, CalendarParticipantRole, CalendarRecurringSlotSuggestion,
    CalendarRecurringSlotsData,
};

pub(crate) fn find(
    account_id: String,
    plan: &RankedPlan,
    records: &[Record],
    input: &CalendarFindRecurringSlotsInput,
) -> Result<CalendarRecurringSlotsData> {
    let limit = crate::sanitize::limit(input.schedule.limit.map(u32::from), 5, 10)?;
    let times = candidate_ranges(plan, input.schedule.duration_minutes)
        .iter()
        .map(|range| range.start.with_timezone(&plan.schedule.time_zone).time())
        .collect::<BTreeSet<_>>();
    let mut patterns = times
        .into_iter()
        .filter_map(|start| pattern(plan, records, input, start))
        .collect::<Vec<_>>();
    patterns.sort_by_key(rank);
    let results_truncated = patterns.len() > limit;
    patterns.truncate(limit);
    let data = CalendarRecurringSlotsData {
        account_id,
        time_zone: plan.schedule.time_zone.name().into(),
        weekday: input.weekday,
        duration_minutes: input.schedule.duration_minutes,
        buffer_minutes: input.schedule.buffer_minutes,
        precision_minutes: 30,
        resolution_complete: resolved(records),
        participants: records.iter().map(|record| record.summary.clone()).collect(),
        suggestions: patterns,
        results_truncated,
    };
    bounded(&data)?;
    Ok(data)
}

fn pattern(
    plan: &RankedPlan,
    records: &[Record],
    input: &CalendarFindRecurringSlotsInput,
    local_start: NaiveTime,
) -> Option<CalendarRecurringSlotSuggestion> {
    let dates = plan
        .schedule
        .working
        .iter()
        .map(|range| range.start.with_timezone(&plan.schedule.time_zone).date_naive())
        .collect::<BTreeSet<_>>();
    let mut occurrences = Vec::new();
    for date in dates {
        // Reject the entire wall-clock pattern if any occurrence falls into a DST fold or gap.
        let start = time::local_to_utc(plan.schedule.time_zone, date.and_time(local_start)).ok()?;
        let range = UtcInterval {
            start,
            end: start + Duration::minutes(i64::from(input.schedule.duration_minutes)),
        };
        if !evaluate::contained(range, &plan.schedule.working) {
            return None;
        }
        occurrences.push(evaluate::suggestion(plan, records, range, &input.schedule));
    }
    let required_available_occurrences =
        u8::try_from(occurrences.iter().filter(|item| evaluate::required_safe(item)).count())
            .ok()?;
    Some(CalendarRecurringSlotSuggestion {
        local_start_time: local_start.format("%H:%M").to_string(),
        required_available_occurrences,
        occurrences,
    })
}

fn rank(
    value: &CalendarRecurringSlotSuggestion,
) -> (Reverse<u8>, usize, usize, usize, usize, usize, String) {
    let mut required = 0;
    let mut required_unknown = 0;
    let mut optional = 0;
    let mut optional_unknown = 0;
    let mut tentative = 0;
    for occurrence in &value.occurrences {
        let (issues, unknown) = evaluate::counts(occurrence, CalendarParticipantRole::Required);
        required += issues;
        required_unknown += unknown;
        let (issues, unknown) = evaluate::counts(occurrence, CalendarParticipantRole::Optional);
        optional += issues;
        optional_unknown += unknown;
        tentative += occurrence.tentative_participants.len();
    }
    (
        Reverse(value.required_available_occurrences),
        required,
        required_unknown,
        optional,
        optional_unknown,
        tentative,
        value.local_start_time.clone(),
    )
}
