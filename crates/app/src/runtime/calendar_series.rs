pub(super) mod edit;
mod exception_patch;
mod exceptions;
mod occurrence;
pub(super) mod preview;
pub(super) mod response;
pub(super) mod rule;
mod split;
#[cfg(test)]
mod tests;

pub(super) use exceptions::validate;
pub(super) use occurrence::{selected, validate_member};

use chrono::{DateTime, Utc};
use eas_mail_protocol::{CalendarApplication, CalendarProperties, CalendarRecurrence, Patch};

use super::calendar_agenda::timezone::EventTimeZone;
use super::calendar_prepare::PreparedEvent;
use crate::backend::{BackendCalendarMutation, BackendEvent};
use crate::{AppError, ErrorCode, Result};

pub(super) fn properties(source: &BackendEvent) -> Result<CalendarProperties> {
    let properties = read_properties(source)?;
    if !properties.can_write() || matches!(source.fields.body_truncated, Patch::Value(true)) {
        return Err(invalid(
            "Calendar data contains unsupported or truncated fields; write blocked",
        ));
    }
    Ok(properties)
}

pub(super) fn read_properties(source: &BackendEvent) -> Result<CalendarProperties> {
    let mut properties = source.fields.properties.clone().unwrap_or_default();
    if properties.recurrence.is_none()
        && let Patch::Value(fields) = &source.fields.recurrence
        && !fields.is_empty()
    {
        properties.recurrence = Some(
            CalendarRecurrence::from_fields(fields)
                .map_err(|_| invalid("Calendar recurrence cannot be preserved safely"))?,
        );
    }
    if source.fields.properties.is_none()
        && matches!(&source.fields.exceptions, Patch::Value(values) if !values.is_empty())
    {
        return Err(invalid("Calendar exceptions lack lossless metadata; refetch the master"));
    }
    Ok(properties)
}

pub(super) fn prepared(application: CalendarApplication) -> Result<PreparedEvent> {
    let all_day_dates = if application.all_day {
        let zone = zone(&application)?;
        Some((
            zone.to_local(application.starts_at)?.date(),
            zone.to_local(application.ends_at)?.date(),
        ))
    } else {
        None
    };
    Ok(PreparedEvent {
        mutation: BackendCalendarMutation { target_collection: None, application },
        all_day_dates,
    })
}

pub(super) fn zone(application: &CalendarApplication) -> Result<EventTimeZone> {
    EventTimeZone::parse(Some(&application.time_zone), chrono_tz::UTC)
}

pub(super) fn original_time(source: &BackendEvent) -> Result<DateTime<Utc>> {
    source
        .occurrence_start
        .ok_or_else(|| invalid("occurrence and following require an agenda occurrence reference"))
}

pub(super) fn invalid(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}

pub(super) fn stale() -> AppError {
    AppError::new(
        ErrorCode::SyncStale,
        "The selected original occurrence no longer belongs to this series",
    )
}

pub(super) fn revision(source: &BackendEvent) -> Result<String> {
    use sha2::{Digest as _, Sha256};
    let bytes = serde_json::to_vec(&source.fields)
        .map_err(|_| invalid("cannot fingerprint Calendar source"))?;
    Ok(Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect())
}
