use eas_mail_protocol::{CalendarApplication, CalendarFields, Patch};

use super::super::BackendEvent;
use super::session::{EasMailbox, SessionState};
use crate::{AppError, ErrorCode, Result};

pub(super) fn backend_event(
    mailbox: &EasMailbox,
    collection_id: String,
    server_id: String,
    item: &CalendarApplication,
) -> BackendEvent {
    BackendEvent {
        occurrence_start: None,
        account_id: mailbox.account.account_id.clone(),
        long_id: String::new(),
        collection_id: Some(collection_id),
        server_id: Some(server_id),
        fields: application_fields(item),
    }
}

fn application_fields(item: &CalendarApplication) -> CalendarFields {
    CalendarFields {
        properties: Some(item.properties.clone()),
        recurrence: Patch::Value(
            item.properties
                .recurrence
                .as_ref()
                .map(eas_mail_protocol::CalendarRecurrence::to_fields)
                .unwrap_or_default(),
        ),
        exceptions: Patch::Value(
            item.properties
                .exceptions
                .iter()
                .map(eas_mail_protocol::protocol::exception_fields)
                .collect(),
        ),
        subject: Patch::Value(item.subject.clone()),
        body: Patch::Value(item.body.clone()),
        body_truncated: Patch::Value(false),
        starts_at: Patch::Value(Some(item.starts_at)),
        ends_at: Patch::Value(Some(item.ends_at)),
        all_day: Patch::Value(item.all_day),
        location: Patch::Value(item.location.clone()),
        attendees: Patch::Value(item.attendees.clone()),
        reminder_minutes: Patch::Value(item.reminder_minutes),
        meeting_status: Patch::Value(item.meeting_status),
        uid: Patch::Value(item.uid.clone()),
        dt_stamp: Patch::Value(Some(item.dt_stamp)),
        time_zone: Patch::Value(item.time_zone.clone()),
        busy_status: Patch::Value(item.busy_status),
        response_requested: Patch::Value(item.response_requested),
        ..CalendarFields::default()
    }
}

pub(super) fn source_ids(source: &BackendEvent) -> Result<(&str, &str)> {
    match (source.collection_id.as_deref(), source.server_id.as_deref()) {
        (Some(collection), Some(server)) => Ok((collection, server)),
        _ => Err(AppError::new(ErrorCode::NotFound, "Calendar mutable source is unavailable")
            .account(&source.account_id)),
    }
}

pub(super) fn calendar_change_payload(
    source: &BackendEvent,
    item: &CalendarApplication,
) -> CalendarApplication {
    let mut payload = item.clone();
    if let Some(previous) = &source.fields.properties {
        // MS-ASCMD 2.2.3.24: omitted Exception nodes remain unchanged in Calendar Sync/Change.
        // Keep the complete local result, but do not replay server-expanded sibling overrides.
        payload.properties.exceptions.retain_mut(|exception| {
            let prior = previous
                .exceptions
                .iter()
                .find(|prior| prior.original_start == exception.original_start);
            if prior.is_some_and(|prior| exception == prior) {
                return false;
            }
            // In EAS 14.1 Change, an omitted in-schema property is actively deleted. Express
            // empty categories this way: replaying the server's empty Categories container
            // inside an Exception is rejected by supported providers with item status 6.
            if let Some(properties) = &mut exception.fields.properties
                && properties.categories.as_ref().is_some_and(Vec::is_empty)
            {
                properties.categories = None;
            }
            true
        });
    }
    payload
}

pub(super) fn calendar_filter(state: &SessionState) -> Result<u8> {
    state.policy.as_ref().map(|value| value.calendar_filter_type).ok_or_else(|| {
        AppError::new(ErrorCode::ProtocolError, "Calendar policy state is unavailable")
    })
}

pub(super) fn current_calendar_key(state: &SessionState, collection_id: &str) -> Result<String> {
    state
        .collections
        .get(collection_id)
        .map(|value| value.sync_key.clone())
        .filter(|value| value != "0")
        .ok_or_else(|| {
            AppError::new(ErrorCode::SyncStale, "Calendar SyncKey is unavailable after recovery")
        })
}

pub(super) fn missing_during(action: &str, account_id: &str) -> AppError {
    AppError::new(ErrorCode::NotFound, format!("Calendar item disappeared during {action}"))
        .account(account_id)
}

pub(super) fn patch_eq(value: &Patch<String>, expected: &str) -> bool {
    matches!(value, Patch::Value(value) if value == expected)
}

pub(super) fn required_string<'a>(
    value: &'a Patch<String>,
    message: &'static str,
) -> Result<&'a str> {
    match value {
        Patch::Value(value) if !value.is_empty() => Ok(value),
        _ => Err(AppError::new(ErrorCode::ProtocolError, message)),
    }
}

pub(super) fn validate_mutation(
    result: eas_mail_protocol::MutationResult,
) -> Result<eas_mail_protocol::MutationResult> {
    require_status(result.status, "Calendar Sync")?;
    Ok(result)
}

pub(super) fn require_status(status: u16, command: &str) -> Result<()> {
    if status == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::ProtocolError,
            format!("Exchange rejected {command} with status {status}"),
        ))
    }
}
