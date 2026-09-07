use serde_json::Value;

use super::output::OutputKind;
use crate::{AppError, ErrorCode, Result};

pub(super) fn render(value: &Value, kind: OutputKind) -> Result<String> {
    match kind {
        OutputKind::Accounts => list(value, "/accounts", account_line),
        OutputKind::Folders => list(value, "/folders", folder_line),
        OutputKind::People => list(value, "/items", |person| {
            format!("{}  {}", field(person, "name"), field(person, "email"))
        }),
        OutputKind::MailList => list(value, "/items", mail_line),
        OutputKind::MailDetail => mail_detail(value),
        OutputKind::AutoReply => Ok(auto_reply(value)),
        OutputKind::MailThread => {
            let items = value.get("items").and_then(Value::as_array).ok_or_else(invalid_output)?;
            let body = items.iter().map(mail_detail).collect::<Result<Vec<_>>>()?.join("\n\n");
            Ok(format!(
                "Messages truncated: {}. Bodies truncated: {}.\n\n{body}",
                field(value, "results_truncated"),
                field(value, "bodies_truncated")
            ))
        }
        OutputKind::Attachments => list(value, "/attachments", attachment_line),
        OutputKind::Download => download(value),
        OutputKind::Availability => availability(value),
        OutputKind::Slots => super::human_slots::render(value, false),
        OutputKind::RecurringSlots => super::human_slots::render(value, true),
        OutputKind::CalendarList => list(value, "/items", event_line),
        OutputKind::CalendarEvent => calendar_event(value),
        OutputKind::Write => write_result(value),
        OutputKind::Bulk => serde_json::to_string_pretty(value).map_err(|_| invalid_output()),
    }
}

fn list(value: &Value, pointer: &str, formatter: fn(&Value) -> String) -> Result<String> {
    let values = value.pointer(pointer).and_then(Value::as_array).ok_or_else(invalid_output)?;
    if values.is_empty() {
        return Ok("No results".into());
    }
    Ok(values.iter().map(formatter).collect::<Vec<_>>().join("\n"))
}

fn account_line(value: &Value) -> String {
    format!("{}  {}  {}", field(value, "account_id"), field(value, "email"), field(value, "status"))
}

fn folder_line(value: &Value) -> String {
    format!(
        "{}  {}  {}  {}",
        field(value, "account_id"),
        field(value, "role"),
        field(value, "folder_id"),
        field(value, "display_name")
    )
}

fn mail_line(value: &Value) -> String {
    format!(
        "{}  {}  {}\n  from: {}\n  ref: {}",
        field(value, "received_at"),
        read_marker(value),
        field(value, "subject"),
        field(value, "sender"),
        field(value, "mail_ref")
    )
}

fn mail_detail(value: &Value) -> Result<String> {
    Ok(format!(
        "{}\nFrom: {}\nTo: {}\nCc: {}\nReceived: {}\nReference: {}\n\n{}",
        field(value, "subject"),
        field(value, "sender"),
        field(value, "recipients"),
        field(value, "cc"),
        field(value, "received_at"),
        field(value, "mail_ref"),
        field(value, "body")
    ))
}

fn auto_reply(value: &Value) -> String {
    let mut lines = vec![format!(
        "Account: {}\nState: {}\nStart: {}\nEnd: {}",
        field(value, "account_id"),
        field(value, "state"),
        field(value, "starts_at"),
        field(value, "ends_at")
    )];
    for (key, label) in [
        ("internal", "Internal"),
        ("external_known", "External contacts"),
        ("external_unknown", "Other external senders"),
    ] {
        if let Some(message) = value.get(key).filter(|value| !value.is_null()) {
            lines.push(format!(
                "{label}: enabled {}\n{}",
                field(message, "enabled"),
                field(message, "message")
            ));
        } else {
            lines.push(format!("{label}: not returned by Exchange"));
        }
    }
    lines.join("\n\n")
}

fn attachment_line(value: &Value) -> String {
    format!(
        "{}  {} bytes  {}\n  ref: {}",
        field(value, "display_name"),
        field(value, "size"),
        field(value, "content_type"),
        field(value, "attachment_ref")
    )
}

fn download(value: &Value) -> Result<String> {
    Ok(format!("{}\nexpires: {}", field(value, "path"), field(value, "expires_at")))
}

fn availability(value: &Value) -> Result<String> {
    let participants =
        value.get("participants").and_then(Value::as_array).ok_or_else(invalid_output)?;
    let mut lines = vec![format!(
        "{} to {}  {}  precision {} min",
        field(value, "date_from"),
        field(value, "date_to"),
        field(value, "time_zone"),
        field(value, "precision_minutes")
    )];
    for participant in participants {
        lines.push(format!(
            "{}  {}  {}",
            field(participant, "input"),
            field(participant, "resolution"),
            field(participant, "availability")
        ));
        if let Some(intervals) = participant.get("intervals").and_then(Value::as_array) {
            lines.extend(intervals.iter().map(|interval| {
                format!(
                    "  {} - {}  {}",
                    field(interval, "starts_at"),
                    field(interval, "ends_at"),
                    field(interval, "status")
                )
            }));
        }
    }
    Ok(lines.join("\n"))
}

fn event_line(value: &Value) -> String {
    format!(
        "{} - {}  {}\n  location: {}\n  ref: {}",
        field(value, "starts_at"),
        field(value, "ends_at"),
        field(value, "subject"),
        field(value, "location"),
        field(value, "event_ref")
    )
}

fn calendar_event(value: &Value) -> Result<String> {
    Ok(format!(
        "{}\n{} - {}\nLocation: {}\nOrganizer: {}\nType: {}\nReference: {}\n\n{}",
        field(value, "subject"),
        field(value, "starts_at"),
        field(value, "ends_at"),
        field(value, "location"),
        field(value, "organizer"),
        field(value, "event_type"),
        field(value, "event_ref"),
        field(value, "body")
    ))
}

fn write_result(value: &Value) -> Result<String> {
    let mut lines =
        vec![format!("{}  operation {}", field(value, "status"), field(value, "operation_id"))];
    if value.get("message").is_some() {
        lines.push(field(value, "message"));
    }
    if value.get("event_ref").is_some_and(|item| !item.is_null()) {
        lines.push(format!("event ref: {}", field(value, "event_ref")));
    }
    Ok(lines.join("\n"))
}

fn read_marker(value: &Value) -> &'static str {
    if value.get("is_read").and_then(Value::as_bool) == Some(true) { "read" } else { "unread" }
}

fn field(value: &Value, name: &str) -> String {
    let Some(value) = value.get(name) else {
        return "-".into();
    };
    match value {
        Value::Null => "-".into(),
        Value::String(value) => literal(value),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "-".into()),
    }
}

fn literal(value: &str) -> String {
    value.chars().flat_map(|character| character.escape_default()).collect()
}

fn invalid_output() -> AppError {
    AppError::new(ErrorCode::ProtocolError, "CLI received an unexpected runtime response")
}
