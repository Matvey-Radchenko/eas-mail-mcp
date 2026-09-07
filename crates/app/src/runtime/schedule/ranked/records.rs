use chrono::Duration;
use eas_mail_protocol::{CandidateAvailability, RecipientAvailability};

use super::super::{
    StatusInterval, UtcInterval, availability_state, free_busy_state, merge_candidates,
    merge_status_intervals, protocol, resolution_state,
};
use crate::Result;
use crate::model::{
    CalendarSlotParticipant, FreeBusyState, ParticipantAvailabilityState,
    ParticipantResolutionState,
};

pub(crate) struct Page {
    pub(crate) range: UtcInterval,
    pub(crate) participants: Option<Vec<RecipientAvailability>>,
}

pub(crate) struct Record {
    pub(crate) summary: CalendarSlotParticipant,
    pub(super) intervals: Vec<StatusInterval>,
    observed: bool,
}

pub(crate) fn prepare(requested: &[String], pages: Vec<Page>) -> Result<Vec<Record>> {
    let mut records = requested
        .iter()
        .map(|input| Record {
            summary: CalendarSlotParticipant {
                input: input.trim().into(),
                resolution: ParticipantResolutionState::NotFound,
                display_name: None,
                email: None,
                availability: ParticipantAvailabilityState::Missing,
                candidates: Vec::new(),
                has_no_data: false,
                untrusted_external_content: true,
            },
            intervals: Vec::new(),
            observed: false,
        })
        .collect::<Vec<_>>();
    for page in pages {
        let Some(participants) = page.participants else {
            for record in &mut records {
                record.summary.has_no_data = true;
                record
                    .intervals
                    .push(StatusInterval { range: page.range, status: FreeBusyState::NoData });
            }
            continue;
        };
        if participants.len() != requested.len() {
            return Err(protocol("ResolveRecipients returned an unexpected participant count"));
        }
        for ((record, response), input) in records.iter_mut().zip(participants).zip(requested) {
            merge(record, input, page.range, response)?;
        }
    }
    for record in &mut records {
        record.intervals = merge_status_intervals(std::mem::take(&mut record.intervals))?;
        record.summary.has_no_data |=
            record.intervals.iter().any(|item| item.status == FreeBusyState::NoData);
    }
    Ok(records)
}

fn merge(
    record: &mut Record,
    input: &str,
    range: UtcInterval,
    response: RecipientAvailability,
) -> Result<()> {
    if response.input.trim() != input.trim() {
        return Err(protocol("ResolveRecipients changed participant ordering"));
    }
    let resolution = resolution_state(response.resolution);
    if record.observed && record.summary.resolution != resolution {
        return Err(protocol("recipient resolution changed between availability pages"));
    }
    record.observed = true;
    record.summary.resolution = resolution;
    if resolution != ParticipantResolutionState::Resolved {
        merge_candidates(&mut record.summary.candidates, response.candidates);
        record.summary.has_no_data = true;
        record.intervals.push(StatusInterval { range, status: FreeBusyState::NoData });
        return Ok(());
    }
    if response.candidates.len() != 1 {
        return Err(protocol("resolved recipient must have exactly one candidate"));
    }
    let candidate = response
        .candidates
        .into_iter()
        .next()
        .ok_or_else(|| protocol("resolved recipient is missing"))?;
    if record
        .summary
        .email
        .as_ref()
        .is_some_and(|email| !email.eq_ignore_ascii_case(&candidate.email))
    {
        return Err(protocol("resolved recipient changed between availability pages"));
    }
    record.summary.display_name = Some(candidate.display_name);
    record.summary.email = Some(candidate.email);
    let state = availability_state(&candidate.availability);
    if state != ParticipantAvailabilityState::Available
        || record.summary.availability == ParticipantAvailabilityState::Missing
    {
        record.summary.availability = state;
    }
    let CandidateAvailability::Slots(values) = candidate.availability else {
        record.summary.has_no_data = true;
        record.intervals.push(StatusInterval { range, status: FreeBusyState::NoData });
        return Ok(());
    };
    let expected = ((range.end - range.start).num_seconds() + 1799) / 1800;
    if i64::try_from(values.len()).ok() != Some(expected) {
        return Err(protocol("availability slot count does not cover the requested interval"));
    }
    for (index, status) in values.into_iter().enumerate() {
        let offset =
            i64::try_from(index).map_err(|_| protocol("availability interval overflow"))?;
        let start = range.start + Duration::minutes(offset * 30);
        record.intervals.push(StatusInterval {
            range: UtcInterval { start, end: (start + Duration::minutes(30)).min(range.end) },
            status: free_busy_state(status),
        });
    }
    Ok(())
}
