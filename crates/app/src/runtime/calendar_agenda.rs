pub(in crate::runtime) mod recurrence;
pub(in crate::runtime) mod timezone;

use chrono::{DateTime, LocalResult, NaiveDate, NaiveDateTime, TimeZone as _, Utc};
use chrono_tz::Tz;
use eas_mail_protocol::{CalendarAttendee, Patch};

use crate::backend::BackendEvent;
use crate::model::CalendarSearchInput;
use crate::sanitize::plain_text;
use crate::{AppError, ErrorCode, Result};

const MAX_DAYS: i64 = 31;

#[derive(Debug, Clone)]
pub(super) struct AgendaPlan {
    query: Option<String>,
    range: Option<AgendaRange>,
    time_zone: Option<Tz>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct AgendaRange {
    pub(super) start: DateTime<Utc>,
    pub(super) end: DateTime<Utc>,
}

pub(super) fn plan(input: &CalendarSearchInput) -> Result<AgendaPlan> {
    let query =
        input.query.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned);
    if input.query.is_some() && query.is_none() {
        return Err(validation("calendar search query must not be empty"));
    }
    let (range, time_zone) = match (&input.date_from, &input.date_to, &input.time_zone) {
        (None, None, None) => (None, None),
        (Some(date_from), Some(date_to), Some(time_zone)) => {
            let time_zone = time_zone
                .parse::<Tz>()
                .map_err(|_| validation("time_zone must be a valid IANA timezone"))?;
            (Some(date_range(date_from, date_to, time_zone)?), Some(time_zone))
        }
        _ => {
            return Err(validation("date_from, date_to, and time_zone must be supplied together"));
        }
    };
    if query.is_none() && range.is_none() {
        return Err(validation("calendar search needs query text or a date range"));
    }
    Ok(AgendaPlan { query, range, time_zone })
}

impl AgendaPlan {
    pub(super) const fn uses_agenda_scan(&self) -> bool {
        self.range.is_some()
    }

    pub(super) fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    pub(super) fn apply(&self, events: Vec<BackendEvent>) -> Result<Vec<BackendEvent>> {
        let Some(range) = self.range else {
            return Ok(events);
        };
        let time_zone =
            self.time_zone.ok_or_else(|| protocol("Calendar agenda has no timezone"))?;
        let mut output = Vec::new();
        for event in events {
            output.extend(recurrence::expand(event, range, time_zone)?);
        }
        if let Some(query) = self.query() {
            let query = plain_text(query).to_lowercase();
            output.retain(|event| matches_query(event, &query));
        }
        output.sort_by_key(event_start);
        Ok(output)
    }
}

fn date_range(date_from: &str, date_to: &str, time_zone: Tz) -> Result<AgendaRange> {
    let date_from = parse_date(date_from)?;
    let date_to = parse_date(date_to)?;
    let days = date_to.signed_duration_since(date_from).num_days().saturating_add(1);
    if !(1..=MAX_DAYS).contains(&days) {
        return Err(validation("date range must contain from 1 through 31 days"));
    }
    let exclusive_end =
        date_to.succ_opt().ok_or_else(|| validation("calendar date range overflows"))?;
    Ok(AgendaRange {
        start: local_midnight(time_zone, date_from)?,
        end: local_midnight(time_zone, exclusive_end)?,
    })
}

fn parse_date(value: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| validation("dates must use YYYY-MM-DD format"))
}

fn local_midnight(time_zone: Tz, date: NaiveDate) -> Result<DateTime<Utc>> {
    let local = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| validation("calendar local midnight is invalid"))?;
    local_to_utc(time_zone, local)
}

pub(super) fn local_to_utc(time_zone: Tz, value: NaiveDateTime) -> Result<DateTime<Utc>> {
    match time_zone.from_local_datetime(&value) {
        LocalResult::Single(value) => Ok(value.with_timezone(&Utc)),
        LocalResult::Ambiguous(_, _) => Err(validation("calendar range contains ambiguous time")),
        LocalResult::None => Err(validation("calendar range contains nonexistent time")),
    }
}

fn matches_query(event: &BackendEvent, query: &str) -> bool {
    let fields = &event.fields;
    [
        text(&fields.subject),
        text(&fields.location),
        text(&fields.organizer),
        text(&fields.organizer_email),
    ]
    .into_iter()
    .chain(
        attendees(&fields.attendees).flat_map(|value| [value.name.as_str(), value.email.as_str()]),
    )
    .map(plain_text)
    .any(|value| value.to_lowercase().contains(query))
}

fn attendees(patch: &Patch<Vec<CalendarAttendee>>) -> impl Iterator<Item = &CalendarAttendee> {
    match patch {
        Patch::Value(values) => values.as_slice(),
        Patch::Missing => &[],
    }
    .iter()
}

fn text(patch: &Patch<String>) -> &str {
    match patch {
        Patch::Value(value) => value,
        Patch::Missing => "",
    }
}

fn event_start(event: &BackendEvent) -> Option<DateTime<Utc>> {
    match &event.fields.starts_at {
        Patch::Value(Some(value)) => Some(*value),
        Patch::Missing | Patch::Value(None) => None,
    }
}

fn validation(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}

pub(super) fn protocol(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ProtocolError, message)
}

#[cfg(test)]
mod error_tests;
#[cfg(test)]
mod tests;
