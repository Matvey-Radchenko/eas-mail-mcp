use std::collections::{BTreeMap, BTreeSet};

use chrono::Duration;

use super::super::{SchedulePlan, UtcInterval, time};
use crate::model::{CalendarFindSlotsInput, CalendarParticipantRole, ScheduleWeekday};
use crate::{AppError, ErrorCode, Result};

pub(crate) struct PersonPlan {
    pub(crate) role: CalendarParticipantRole,
    pub(crate) working: Vec<UtcInterval>,
}

pub(crate) struct RankedPlan {
    pub(crate) schedule: SchedulePlan,
    pub(crate) people: Vec<PersonPlan>,
    pub(crate) queries: Vec<UtcInterval>,
}

pub(crate) fn build(
    input: &CalendarFindSlotsInput,
    weekday: Option<ScheduleWeekday>,
) -> Result<RankedPlan> {
    super::super::validate_participants(&input.participants)?;
    super::super::validate_slot_options(
        input.duration_minutes,
        usize::from(input.limit.unwrap_or(20)),
    )?;
    if input.buffer_minutes > 120 || !input.buffer_minutes.is_multiple_of(15) {
        return Err(validation("buffer_minutes must be 0-120 and divisible by 15"));
    }
    let schedule = time::build_plan_with_limit(
        &input.date_from,
        &input.date_to,
        &input.time_zone,
        &input.working_hours,
        if weekday.is_some() { 90 } else { 31 },
        weekday,
    )?;
    let first = schedule
        .working
        .first()
        .ok_or_else(|| validation("no working hours match the requested weekday"))?
        .start;
    let last = schedule
        .working
        .last()
        .ok_or_else(|| validation("no working hours match the requested dates"))?
        .end;
    let people = people(input, &schedule.working)?;
    let buffer = Duration::minutes(i64::from(input.buffer_minutes));
    let queries = if weekday.is_some() {
        weekly_queries(&schedule, buffer)?
    } else {
        time::split_chunks(
            first - buffer,
            (last + buffer).max(first - buffer + Duration::minutes(30)),
        )?
    };
    Ok(RankedPlan { schedule, people, queries })
}

fn people(input: &CalendarFindSlotsInput, requested: &[UtcInterval]) -> Result<Vec<PersonPlan>> {
    let mut seen = BTreeSet::new();
    for participant in &input.participants {
        if !seen.insert(participant.trim()) {
            return Err(validation("participants must not contain duplicates"));
        }
    }
    seen.clear();
    for option in &input.participant_options {
        if !input.participants.iter().any(|value| value.trim() == option.input.trim())
            || !seen.insert(option.input.trim())
        {
            return Err(validation("participant_options must reference unique participant inputs"));
        }
    }
    let mut output = Vec::new();
    for participant in &input.participants {
        let options =
            input.participant_options.iter().find(|value| value.input.trim() == participant.trim());
        let zone = options
            .and_then(|value| value.time_zone.as_deref())
            .unwrap_or(&input.time_zone)
            .parse()
            .map_err(|_| validation("invalid participant IANA timezone"))?;
        let hours = options
            .and_then(|value| value.working_hours.as_deref())
            .unwrap_or(&input.working_hours);
        output.push(PersonPlan {
            role: options.map_or(CalendarParticipantRole::Required, |value| value.role),
            working: time::person_working(requested, zone, hours)?,
        });
    }
    if !output.iter().any(|value| value.role == CalendarParticipantRole::Required) {
        return Err(validation("at least one participant must be required"));
    }
    Ok(output)
}

fn weekly_queries(schedule: &SchedulePlan, buffer: Duration) -> Result<Vec<UtcInterval>> {
    let mut days = BTreeMap::new();
    for interval in &schedule.working {
        let date = interval.start.with_timezone(&schedule.time_zone).date_naive();
        days.entry(date)
            .and_modify(|range: &mut UtcInterval| range.end = range.end.max(interval.end))
            .or_insert(*interval);
    }
    if days.len() > 13 {
        return Err(validation("weekly search supports at most 13 occurrences"));
    }
    Ok(days
        .into_values()
        .map(|range| UtcInterval {
            start: range.start - buffer,
            end: (range.end + buffer).max(range.start - buffer + Duration::minutes(30)),
        })
        .collect())
}

fn validation(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}
