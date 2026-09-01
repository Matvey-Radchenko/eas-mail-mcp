pub(in crate::runtime) mod pattern;

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use chrono_tz::Tz;
use eas_mail_protocol::Patch;

use self::pattern::Pattern;
use super::timezone::EventTimeZone;
use super::{AgendaRange, protocol};
use crate::Result;
use crate::backend::BackendEvent;

const MAX_EVENT_DURATION_DAYS: i64 = 366;

pub(super) fn expand(
    event: BackendEvent,
    range: AgendaRange,
    fallback_zone: Tz,
) -> Result<Vec<BackendEvent>> {
    let recurrence = map(&event.fields.recurrence);
    if recurrence.is_empty() {
        return Ok(overlaps_event(&event, range).then_some(event).into_iter().collect());
    }
    let pattern = Pattern::parse(recurrence)?;
    let starts_at = required_time(&event.fields.starts_at, "Recurring event has no start time")?;
    let ends_at = required_time(&event.fields.ends_at, "Recurring event has no end time")?;
    let elapsed = ends_at.signed_duration_since(starts_at);
    if elapsed <= Duration::zero() || elapsed > Duration::days(MAX_EVENT_DURATION_DAYS) {
        return Err(protocol("Recurring event duration is outside supported bounds"));
    }
    let zone = EventTimeZone::parse(string(&event.fields.time_zone), fallback_zone)?;
    let master_local = zone.to_local(starts_at)?;
    let master_end_local = zone.to_local(ends_at)?;
    let wall_duration = master_end_local.signed_duration_since(master_local);
    let probe_start = range
        .start
        .checked_sub_signed(elapsed)
        .ok_or_else(|| protocol("Calendar agenda range underflowed"))?;
    let first_date = zone
        .to_local(probe_start)?
        .date()
        .pred_opt()
        .ok_or_else(|| protocol("Calendar agenda date underflowed"))?;
    let last_date = zone
        .to_local(range.end)?
        .date()
        .succ_opt()
        .ok_or_else(|| protocol("Calendar agenda date overflowed"))?;
    let mut exceptions = parse_exceptions(&event.fields.exceptions)?;
    let mut output = Vec::new();
    let mut date = first_date;
    loop {
        if let Some(ordinal) = pattern.ordinal(date, master_local.date())?
            && pattern.allows(ordinal)
        {
            let local_start = date.and_time(master_local.time());
            let start = zone.to_utc(local_start)?;
            if pattern.until.is_none_or(|until| start <= until) {
                let local_end = local_start
                    .checked_add_signed(wall_duration)
                    .ok_or_else(|| protocol("Recurring event end overflowed"))?;
                let end = zone.to_utc(local_end)?;
                let exception = exceptions.remove(&start.timestamp());
                if let Some(occurrence) = occurrence(&event, start, end, exception.as_ref())?
                    && overlaps_event(&occurrence, range)
                {
                    output.push(occurrence);
                }
            }
        }
        if date >= last_date {
            break;
        }
        date = date.succ_opt().ok_or_else(|| protocol("Calendar agenda date overflowed"))?;
    }
    for exception in exceptions.into_values() {
        if exception.deleted || exception.start.is_none() {
            continue;
        }
        let occurrence =
            occurrence(&event, exception.original, exception.original + elapsed, Some(&exception))?;
        if let Some(occurrence) = occurrence.filter(|value| overlaps_event(value, range)) {
            output.push(occurrence);
        }
    }
    output.sort_by_key(event_start);
    output.dedup_by_key(|value| event_start(value).map(|time| time.timestamp()));
    Ok(output)
}

struct ExceptionChange {
    original: DateTime<Utc>,
    deleted: bool,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    subject: Option<String>,
    location: Option<String>,
    all_day: Option<bool>,
}

fn parse_exceptions(
    patch: &Patch<Vec<BTreeMap<String, String>>>,
) -> Result<BTreeMap<i64, ExceptionChange>> {
    let mut output = BTreeMap::new();
    for values in list(patch) {
        let original = values
            .get("exceptionstarttime")
            .ok_or_else(|| protocol("Calendar recurrence exception has no original time"))
            .and_then(|value| parse_time(value))?;
        let change = ExceptionChange {
            original,
            deleted: values.get("deleted").is_some_and(|value| value == "1"),
            start: values.get("starttime").map(|value| parse_time(value)).transpose()?,
            end: values.get("endtime").map(|value| parse_time(value)).transpose()?,
            subject: values.get("subject").cloned(),
            location: values.get("location").cloned(),
            all_day: values.get("alldayevent").map(|value| value == "1"),
        };
        output.insert(original.timestamp(), change);
    }
    Ok(output)
}

fn occurrence(
    source: &BackendEvent,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    exception: Option<&ExceptionChange>,
) -> Result<Option<BackendEvent>> {
    if exception.is_some_and(|value| value.deleted) {
        return Ok(None);
    }
    let mut event = source.clone();
    event.occurrence_start = Some(start);
    let duration = end.signed_duration_since(start);
    let start = exception.and_then(|value| value.start).unwrap_or(start);
    let end = exception.and_then(|value| value.end).unwrap_or(start + duration);
    if end <= start {
        return Err(protocol("Calendar recurrence exception has an invalid duration"));
    }
    event.fields.starts_at = Patch::Value(Some(start));
    event.fields.ends_at = Patch::Value(Some(end));
    if let Some(properties) = &source.fields.properties
        && let Some(value) = properties
            .exceptions
            .iter()
            .find(|value| Some(value.original_start) == event.occurrence_start)
    {
        apply_patch(&mut event.fields.attendees, &value.fields.attendees);
        apply_patch(&mut event.fields.busy_status, &value.fields.busy_status);
        apply_patch(&mut event.fields.response_type, &value.fields.response_type);
    }
    if let Some(exception) = exception {
        apply_optional(&mut event.fields.subject, exception.subject.as_ref());
        apply_optional(&mut event.fields.location, exception.location.as_ref());
        if let Some(all_day) = exception.all_day {
            event.fields.all_day = Patch::Value(all_day);
        }
    }
    Ok(Some(event))
}

fn overlaps_event(event: &BackendEvent, range: AgendaRange) -> bool {
    match (event_start(event), event_end(event)) {
        (Some(start), Some(end)) => start < range.end && end > range.start,
        _ => false,
    }
}

fn event_start(event: &BackendEvent) -> Option<DateTime<Utc>> {
    optional_time(&event.fields.starts_at)
}

fn event_end(event: &BackendEvent) -> Option<DateTime<Utc>> {
    optional_time(&event.fields.ends_at)
}

fn required_time(
    patch: &Patch<Option<DateTime<Utc>>>,
    message: &'static str,
) -> Result<DateTime<Utc>> {
    optional_time(patch).ok_or_else(|| protocol(message))
}

fn optional_time(patch: &Patch<Option<DateTime<Utc>>>) -> Option<DateTime<Utc>> {
    match patch {
        Patch::Value(Some(value)) => Some(*value),
        Patch::Missing | Patch::Value(None) => None,
    }
}

fn parse_time(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .or_else(|_| {
            NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%SZ")
                .map(|value| DateTime::from_naive_utc_and_offset(value, Utc))
        })
        .map_err(|_| protocol("Calendar recurrence timestamp is invalid"))
}

fn map(patch: &Patch<BTreeMap<String, String>>) -> &BTreeMap<String, String> {
    match patch {
        Patch::Value(value) => value,
        Patch::Missing => empty_map(),
    }
}

fn empty_map() -> &'static BTreeMap<String, String> {
    static EMPTY: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(BTreeMap::new)
}

fn list(patch: &Patch<Vec<BTreeMap<String, String>>>) -> &[BTreeMap<String, String>] {
    match patch {
        Patch::Value(value) => value,
        Patch::Missing => &[],
    }
}

fn string(patch: &Patch<String>) -> Option<&str> {
    match patch {
        Patch::Value(value) => Some(value),
        Patch::Missing => None,
    }
}

fn apply_optional(target: &mut Patch<String>, value: Option<&String>) {
    if let Some(value) = value {
        *target = Patch::Value(value.clone());
    }
}

fn apply_patch<T: Clone>(target: &mut Patch<T>, patch: &Patch<T>) {
    if let Patch::Value(value) = patch {
        *target = Patch::Value(value.clone());
    }
}
