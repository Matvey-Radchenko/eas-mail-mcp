use chrono::{DateTime, Utc};
use eas_mail_protocol::{CalendarApplication, CalendarFields, Patch};

use crate::model::CalendarUpdateInput;

pub(super) fn merge(
    master: &CalendarApplication,
    original: DateTime<Utc>,
    input: &CalendarUpdateInput,
    result: &CalendarApplication,
) -> CalendarFields {
    let mut fields = master
        .properties
        .exceptions
        .iter()
        .find(|value| value.original_start == original)
        .map(|value| value.fields.clone())
        .unwrap_or_default();
    // Only explicit changes become overrides; unrelated fields continue to inherit the series.
    if input.subject.is_some() {
        fields.subject = Patch::Value(result.subject.clone());
    }
    if input.body.is_some() {
        fields.body = Patch::Value(result.body.clone());
        fields.body_truncated = Patch::Value(false);
    }
    if input.location.is_some() {
        fields.location = Patch::Value(result.location.clone());
    }
    if input.schedule.is_some() {
        fields.starts_at = Patch::Value(Some(result.starts_at));
        fields.ends_at = Patch::Value(Some(result.ends_at));
        fields.all_day = Patch::Value(result.all_day);
    }
    if input.attendees.is_some() {
        fields.attendees = Patch::Value(result.attendees.clone());
        fields.meeting_status = Patch::Value(result.meeting_status);
    }
    if input.clear_reminder {
        fields.reminder_minutes = Patch::Value(None);
    } else if let Some(reminder) = input.reminder_minutes {
        fields.reminder_minutes = Patch::Value(Some(reminder));
    }
    if input.busy_status.is_some() {
        fields.busy_status = Patch::Value(result.busy_status);
    }
    fields.dt_stamp = Patch::Value(Some(result.dt_stamp));
    fields
}
