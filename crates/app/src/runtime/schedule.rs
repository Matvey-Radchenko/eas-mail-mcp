pub(super) mod ranked;
mod slots;
mod time;

use chrono::Duration;
use eas_mail_protocol::{
    CandidateAvailability, FreeBusyStatus, RecipientAvailability, RecipientResolution,
    ResolvedRecipient,
};

use self::time::format_in_zone;
pub(super) use self::time::{SchedulePlan, UtcInterval};
use crate::model::{
    CalendarAvailabilityData, CalendarAvailabilityInterval, CalendarParticipantSchedule,
    CalendarRecipientCandidate, FreeBusyState, ParticipantAvailabilityState,
    ParticipantResolutionState, WorkingHoursInput,
};
use crate::{AppError, ErrorCode, Result};

const PRECISION_MINUTES: u8 = 30;
const MAX_AVAILABILITY_BYTES: usize = 256 * 1024;

pub(super) struct AvailabilityPage {
    pub(super) range: UtcInterval,
    pub(super) participants: Vec<RecipientAvailability>,
}

pub(super) struct PreparedAvailability {
    pub(super) data: CalendarAvailabilityData,
}

#[derive(Clone)]
struct StatusInterval {
    range: UtcInterval,
    status: FreeBusyState,
}

struct Accumulator {
    input: String,
    resolution: Option<ParticipantResolutionState>,
    total_candidates: usize,
    candidates: Vec<CalendarRecipientCandidate>,
    display_name: Option<String>,
    email: Option<String>,
    availability: Option<ParticipantAvailabilityState>,
    intervals: Vec<StatusInterval>,
}

pub(super) fn plan(
    participants: &[String],
    date_from: &str,
    date_to: &str,
    time_zone: &str,
    working_hours: &[WorkingHoursInput],
) -> Result<SchedulePlan> {
    validate_participants(participants)?;
    time::build_plan(date_from, date_to, time_zone, working_hours)
}

pub(super) fn prepare(
    account_id: String,
    requested: &[String],
    plan: &SchedulePlan,
    pages: Vec<AvailabilityPage>,
) -> Result<PreparedAvailability> {
    let mut accumulators =
        requested.iter().map(|input| Accumulator::new(input)).collect::<Vec<_>>();
    for page in pages {
        apply_page(&mut accumulators, requested, plan, page)?;
    }
    let participants = finalize(accumulators, plan)?;
    let resolution_complete =
        participants.iter().all(|value| value.resolution == ParticipantResolutionState::Resolved);
    let data = CalendarAvailabilityData {
        account_id,
        date_from: plan.date_from.to_string(),
        date_to: plan.date_to.to_string(),
        time_zone: plan.time_zone.name().into(),
        precision_minutes: PRECISION_MINUTES,
        resolution_complete,
        participants,
    };
    ensure_bounded(&data)?;
    Ok(PreparedAvailability { data })
}

impl Accumulator {
    fn new(input: &str) -> Self {
        Self {
            input: input.trim().into(),
            resolution: None,
            total_candidates: 0,
            candidates: Vec::new(),
            display_name: None,
            email: None,
            availability: None,
            intervals: Vec::new(),
        }
    }
}

fn apply_page(
    accumulators: &mut [Accumulator],
    requested: &[String],
    plan: &SchedulePlan,
    page: AvailabilityPage,
) -> Result<()> {
    if page.participants.len() != requested.len() || accumulators.len() != requested.len() {
        return Err(protocol("ResolveRecipients returned an unexpected participant count"));
    }
    for ((accumulator, response), input) in
        accumulators.iter_mut().zip(page.participants).zip(requested)
    {
        merge_response(accumulator, input, plan, page.range, response)?;
    }
    Ok(())
}

fn merge_response(
    accumulator: &mut Accumulator,
    input: &str,
    plan: &SchedulePlan,
    range: UtcInterval,
    response: RecipientAvailability,
) -> Result<()> {
    if response.input.trim() != input.trim() {
        return Err(protocol("ResolveRecipients changed participant ordering"));
    }
    let resolution = resolution_state(response.resolution);
    if accumulator.resolution.is_some_and(|value| value != resolution) {
        return Err(protocol("recipient resolution changed between availability pages"));
    }
    accumulator.resolution = Some(resolution);
    accumulator.total_candidates = accumulator.total_candidates.max(response.total_candidates);
    if resolution != ParticipantResolutionState::Resolved {
        merge_candidates(&mut accumulator.candidates, response.candidates);
        accumulator.availability = Some(ParticipantAvailabilityState::Missing);
        return Ok(());
    }
    let candidate = response
        .candidates
        .into_iter()
        .next()
        .ok_or_else(|| protocol("resolved recipient has no candidate"))?;
    merge_identity(accumulator, &candidate)?;
    merge_candidate_availability(accumulator, plan, range, candidate.availability)
}

fn merge_candidate_availability(
    accumulator: &mut Accumulator,
    plan: &SchedulePlan,
    range: UtcInterval,
    availability: CandidateAvailability,
) -> Result<()> {
    let CandidateAvailability::Slots(values) = availability else {
        accumulator.availability = Some(availability_state(&availability));
        accumulator.intervals.clear();
        return Ok(());
    };
    if accumulator
        .availability
        .is_some_and(|value| value != ParticipantAvailabilityState::Available)
    {
        return Ok(());
    }
    accumulator.availability = Some(ParticipantAvailabilityState::Available);
    for (index, status) in values.into_iter().enumerate() {
        let offset = i64::try_from(index).map_err(|_| state_error())?;
        let start = range.start + Duration::minutes(offset.saturating_mul(30));
        let slot =
            UtcInterval { start, end: std::cmp::min(start + Duration::minutes(30), range.end) };
        accumulator.intervals.extend(
            slots::clip_to_working(slot, &plan.working)
                .into_iter()
                .map(|range| StatusInterval { range, status: free_busy_state(status) }),
        );
    }
    Ok(())
}

fn merge_identity(accumulator: &mut Accumulator, candidate: &ResolvedRecipient) -> Result<()> {
    if accumulator.email.as_ref().is_some_and(|value| !value.eq_ignore_ascii_case(&candidate.email))
    {
        return Err(protocol("resolved recipient changed between availability pages"));
    }
    accumulator.display_name = Some(candidate.display_name.clone());
    accumulator.email = Some(candidate.email.clone());
    Ok(())
}

fn merge_candidates(target: &mut Vec<CalendarRecipientCandidate>, values: Vec<ResolvedRecipient>) {
    for value in values.into_iter().take(10) {
        if target.iter().any(|item| item.email.eq_ignore_ascii_case(&value.email)) {
            continue;
        }
        target.push(CalendarRecipientCandidate {
            display_name: value.display_name,
            email: value.email,
            untrusted_external_content: true,
        });
    }
    target.truncate(10);
}

fn finalize(
    accumulators: Vec<Accumulator>,
    plan: &SchedulePlan,
) -> Result<Vec<CalendarParticipantSchedule>> {
    let mut participants = Vec::new();
    for mut value in accumulators {
        let resolution = value.resolution.ok_or_else(state_error)?;
        let availability = value.availability.unwrap_or(ParticipantAvailabilityState::Missing);
        value.intervals = merge_status_intervals(value.intervals)?;
        let intervals = value
            .intervals
            .iter()
            .map(|item| CalendarAvailabilityInterval {
                starts_at: format_in_zone(item.range.start, plan.time_zone),
                ends_at: format_in_zone(item.range.end, plan.time_zone),
                status: item.status,
            })
            .collect();
        participants.push(CalendarParticipantSchedule {
            input: value.input,
            resolution,
            display_name: value.display_name,
            email: value.email,
            total_candidates: u32::try_from(value.total_candidates).unwrap_or(u32::MAX),
            candidates: value.candidates,
            availability,
            intervals,
            untrusted_external_content: true,
        });
    }
    Ok(participants)
}

fn merge_status_intervals(mut values: Vec<StatusInterval>) -> Result<Vec<StatusInterval>> {
    values.sort_by_key(|value| value.range.start);
    let mut output: Vec<StatusInterval> = Vec::new();
    for value in values {
        if let Some(last) = output.last_mut()
            && value.range.start < last.range.end
            && value.status != last.status
        {
            return Err(protocol("availability pages contain overlapping statuses"));
        }
        if let Some(last) = output.last_mut()
            && value.status == last.status
            && value.range.start <= last.range.end
        {
            last.range.end = std::cmp::max(last.range.end, value.range.end);
        } else {
            output.push(value);
        }
    }
    Ok(output)
}

fn validate_participants(participants: &[String]) -> Result<()> {
    if participants.is_empty()
        || participants.len() > 20
        || participants.iter().any(|value| {
            value.trim().is_empty() || value.len() > 254 || value.chars().any(char::is_control)
        })
    {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "participants must contain from 1 through 20 valid names or email addresses",
        ));
    }
    Ok(())
}

pub(super) fn validate_slot_options(duration_minutes: u16, limit: usize) -> Result<()> {
    if !(15..=480).contains(&duration_minutes) || !duration_minutes.is_multiple_of(15) {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "duration_minutes must be 15-480 and divisible by 15",
        ));
    }
    if !(1..=50).contains(&limit) {
        return Err(AppError::new(ErrorCode::ValidationFailed, "limit must be 1-50"));
    }
    Ok(())
}

fn ensure_bounded(value: &CalendarAvailabilityData) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| state_error())?;
    if bytes.len() > MAX_AVAILABILITY_BYTES {
        return Err(AppError::new(
            ErrorCode::ResultTooLarge,
            "calendar availability exceeds 256 KiB; shorten the date range or participant list",
        ));
    }
    Ok(())
}

const fn resolution_state(value: RecipientResolution) -> ParticipantResolutionState {
    match value {
        RecipientResolution::Resolved => ParticipantResolutionState::Resolved,
        RecipientResolution::Ambiguous => ParticipantResolutionState::Ambiguous,
        RecipientResolution::AmbiguousPartial => ParticipantResolutionState::AmbiguousPartial,
        RecipientResolution::NotFound => ParticipantResolutionState::NotFound,
    }
}

const fn free_busy_state(value: FreeBusyStatus) -> FreeBusyState {
    match value {
        FreeBusyStatus::Free => FreeBusyState::Free,
        FreeBusyStatus::Tentative => FreeBusyState::Tentative,
        FreeBusyStatus::Busy => FreeBusyState::Busy,
        FreeBusyStatus::OutOfOffice => FreeBusyState::OutOfOffice,
        FreeBusyStatus::NoData => FreeBusyState::NoData,
    }
}

const fn availability_state(value: &CandidateAvailability) -> ParticipantAvailabilityState {
    match value {
        CandidateAvailability::Slots(_) => ParticipantAvailabilityState::Available,
        CandidateAvailability::TooManyRecipients => ParticipantAvailabilityState::TooManyRecipients,
        CandidateAvailability::DistributionListTooLarge => {
            ParticipantAvailabilityState::DistributionListTooLarge
        }
        CandidateAvailability::TransientFailure => ParticipantAvailabilityState::TransientFailure,
        CandidateAvailability::Failure => ParticipantAvailabilityState::Failure,
        CandidateAvailability::Missing => ParticipantAvailabilityState::Missing,
    }
}

fn protocol(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ProtocolError, message)
}

fn state_error() -> AppError {
    AppError::new(ErrorCode::ProtocolError, "calendar schedule state is inconsistent")
}

#[cfg(test)]
mod tests;
