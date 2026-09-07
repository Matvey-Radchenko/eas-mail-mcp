use chrono::Duration;

use super::super::{UtcInterval, slots, time::format_in_zone};
use super::{PersonPlan, RankedPlan, Record};
use crate::model::{
    CalendarFindSlotsInput, CalendarFreeWindow, CalendarParticipantRole, CalendarSlotConflict,
    CalendarSlotConflictReason as Reason, CalendarSlotSuggestion, FreeBusyState,
    ParticipantResolutionState,
};

pub(super) fn suggestion(
    plan: &RankedPlan,
    records: &[Record],
    range: UtcInterval,
    input: &CalendarFindSlotsInput,
) -> CalendarSlotSuggestion {
    let mut conflicts = Vec::new();
    let mut tentative_participants = Vec::new();
    for (person, record) in plan.people.iter().zip(records) {
        let (reasons, tentative) = assess(person, record, range, input);
        if !reasons.is_empty() {
            conflicts.push(CalendarSlotConflict {
                input: record.summary.input.clone(),
                role: person.role,
                reasons,
            });
        }
        if tentative {
            tentative_participants.push(record.summary.input.clone());
        }
    }
    CalendarSlotSuggestion {
        starts_at: format_in_zone(range.start, plan.schedule.time_zone),
        ends_at: format_in_zone(range.end, plan.schedule.time_zone),
        conflicts,
        tentative_participants,
    }
}

fn assess(
    person: &PersonPlan,
    record: &Record,
    range: UtcInterval,
    input: &CalendarFindSlotsInput,
) -> (Vec<Reason>, bool) {
    let mut reasons = Vec::new();
    if !contained(range, &person.working) {
        reasons.push(Reason::OutsideWorkingHours);
    }
    let buffer = Duration::minutes(i64::from(input.buffer_minutes));
    let padded = UtcInterval { start: range.start - buffer, end: range.end + buffer };
    let mut covered = Vec::new();
    let mut tentative = false;
    for interval in &record.intervals {
        if interval.range.start >= padded.end || interval.range.end <= padded.start {
            continue;
        }
        covered.push(interval.range);
        let reason = match interval.status {
            FreeBusyState::Free => None,
            FreeBusyState::Tentative if input.allow_tentative => {
                tentative = true;
                None
            }
            FreeBusyState::Tentative => Some(Reason::Tentative),
            FreeBusyState::Busy => Some(Reason::Busy),
            FreeBusyState::OutOfOffice => Some(Reason::OutOfOffice),
            FreeBusyState::NoData => Some(Reason::Unknown),
        };
        if let Some(reason) = reason {
            add_reason(&mut reasons, reason);
        }
    }
    if record.summary.resolution != ParticipantResolutionState::Resolved
        || !contained(padded, &slots::merge_intervals(covered))
    {
        add_reason(&mut reasons, Reason::Unknown);
    }
    (reasons, tentative)
}

fn add_reason(reasons: &mut Vec<Reason>, reason: Reason) {
    if !reasons.contains(&reason) {
        reasons.push(reason);
    }
}

pub(super) fn contained(range: UtcInterval, working: &[UtcInterval]) -> bool {
    working.iter().any(|work| work.start <= range.start && work.end >= range.end)
}

pub(super) fn required_safe(value: &CalendarSlotSuggestion) -> bool {
    !value.conflicts.iter().any(|conflict| conflict.role == CalendarParticipantRole::Required)
}

pub(super) fn counts(
    value: &CalendarSlotSuggestion,
    role: CalendarParticipantRole,
) -> (usize, usize) {
    let conflicts =
        value.conflicts.iter().filter(|conflict| conflict.role == role).collect::<Vec<_>>();
    (
        conflicts.len(),
        conflicts.iter().filter(|conflict| conflict.reasons.contains(&Reason::Unknown)).count(),
    )
}

pub(super) fn windows(
    plan: &RankedPlan,
    records: &[Record],
    input: &CalendarFindSlotsInput,
    limit: usize,
) -> Vec<CalendarFreeWindow> {
    let buffer = Duration::minutes(i64::from(input.buffer_minutes));
    let mut groups = vec![plan.schedule.working.clone()];
    for (person, record) in plan.people.iter().zip(records) {
        let free = slots::merge_intervals(
            record
                .intervals
                .iter()
                .filter(|interval| {
                    record.summary.resolution == ParticipantResolutionState::Resolved
                        && (interval.status == FreeBusyState::Free
                            || (input.allow_tentative
                                && interval.status == FreeBusyState::Tentative))
                })
                .map(|interval| interval.range)
                .collect(),
        );
        let eroded = free
            .into_iter()
            .filter_map(|range| {
                let range = UtcInterval { start: range.start + buffer, end: range.end - buffer };
                (range.start < range.end).then_some(range)
            })
            .collect();
        groups.push(slots::intersect_all(&[eroded, person.working.clone()]));
    }
    slots::fitting(slots::intersect_all(&groups), input.duration_minutes, limit)
        .into_iter()
        .map(|range| CalendarFreeWindow {
            window_start: format_in_zone(range.start, plan.schedule.time_zone),
            window_end: format_in_zone(range.end, plan.schedule.time_zone),
            latest_start: format_in_zone(
                range.end - Duration::minutes(i64::from(input.duration_minutes)),
                plan.schedule.time_zone,
            ),
        })
        .collect()
}
