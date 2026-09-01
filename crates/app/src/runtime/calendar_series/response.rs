use chrono::{DateTime, Utc};

use super::{invalid, prepared, selected};
use crate::Result;
use crate::backend::BackendEvent;
use crate::model::{CalendarRespondInput, CalendarScope};
use crate::runtime::calendar_prepare::{self, PreparedEvent};

pub(in crate::runtime) fn prepare(
    source: &mut BackendEvent,
    input: &CalendarRespondInput,
    now: DateTime<Utc>,
) -> Result<PreparedEvent> {
    if input.scope == Some(CalendarScope::Following) {
        return Err(invalid("respond supports only series or occurrence scope"));
    }
    let master = calendar_prepare::existing(source, now)?;
    let scope = super::edit::resolve_scope(input.scope, source, &master.mutation.application)?;
    if scope == CalendarScope::Occurrence {
        prepared(selected(&master.mutation.application, super::original_time(source)?)?)
    } else {
        source.occurrence_start = None;
        Ok(master)
    }
}

pub(in crate::runtime) fn for_read(
    source: BackendEvent,
    original: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<BackendEvent> {
    let Some(original) = original else {
        return Ok(source);
    };
    let master = calendar_prepare::from_fields(&source, now, super::read_properties(&source)?)?;
    let event = selected(&master.mutation.application, original)?;
    let mut output = super::edit::projected(&source, &event);
    output.occurrence_start = Some(original);
    output.fields.recurrence = source.fields.recurrence;
    output.fields.exceptions = source.fields.exceptions;
    output.fields.body_truncated = source.fields.body_truncated;
    output.fields.response_type = source.fields.response_type;
    if let Some(properties) = &source.fields.properties
        && let Some(exception) = properties.exceptions.iter().find(|e| e.original_start == original)
    {
        if !matches!(exception.fields.response_type, eas_mail_protocol::Patch::Missing) {
            output.fields.response_type = exception.fields.response_type.clone();
        }
        if !matches!(exception.fields.body_truncated, eas_mail_protocol::Patch::Missing) {
            output.fields.body_truncated = exception.fields.body_truncated.clone();
        }
    }
    output.fields.properties = source.fields.properties;
    Ok(output)
}
