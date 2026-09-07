use chrono::{DateTime, Utc};
use eas_mail_protocol::{OofAudience, OofMessage, OofSettings, OofState};

use super::write_preview::WritePreview;
use crate::{
    AppError, AutoReplyExternalAudience, AutoReplyMessage, AutoReplyOperationResult,
    AutoReplyOperationState, AutoReplySetInput, AutoReplySettings, AutoReplyState, ErrorCode,
    JournalRecord, OperationStatus, Result,
};

pub(super) fn requested(input: &AutoReplySetInput, now: DateTime<Utc>) -> Result<OofSettings> {
    let state = match input.state {
        AutoReplyState::Disabled => OofState::Disabled,
        AutoReplyState::Enabled => OofState::Enabled,
        AutoReplyState::Scheduled => OofState::Scheduled,
    };
    let valid_dates = if state == OofState::Scheduled {
        input.starts_at.zip(input.ends_at).is_some_and(|(start, end)| start < end && end > now)
    } else {
        input.starts_at.is_none() && input.ends_at.is_none()
    };
    if !valid_dates {
        return Err(invalid(
            "scheduled replies require ordered start and future end timestamps; other states do not accept dates",
        ));
    }
    if state == OofState::Disabled {
        if input.internal_message.is_some()
            || input.external_message.is_some()
            || input.external_audience != AutoReplyExternalAudience::None
        {
            return Err(invalid(
                "disabling automatic replies does not accept message or audience changes",
            ));
        }
        return Ok(OofSettings { state, starts_at: None, ends_at: None, messages: Vec::new() });
    }
    let internal = required_message(input.internal_message.as_deref())?;
    let external = if input.external_audience == AutoReplyExternalAudience::None {
        if input.external_message.is_some() {
            return Err(invalid("external_message requires an external audience"));
        }
        None
    } else {
        Some(required_message(input.external_message.as_deref())?)
    };
    Ok(OofSettings {
        state,
        starts_at: input.starts_at,
        ends_at: input.ends_at,
        messages: vec![
            OofMessage {
                audience: OofAudience::Internal,
                enabled: true,
                message: Some(internal),
                is_html: false,
            },
            OofMessage {
                audience: OofAudience::ExternalKnown,
                enabled: external.is_some(),
                message: external.clone(),
                is_html: false,
            },
            OofMessage {
                audience: OofAudience::ExternalUnknown,
                enabled: input.external_audience == AutoReplyExternalAudience::All,
                message: external,
                is_html: false,
            },
        ],
    })
}

fn required_message(value: Option<&str>) -> Result<String> {
    let value = value.filter(|value| !value.trim().is_empty()).ok_or_else(|| {
        invalid("a non-empty reply message is required for each enabled audience")
    })?;
    if value.chars().count() > 10_000
        || value
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(invalid(
            "reply messages must contain at most 10000 characters without control sequences",
        ));
    }
    Ok(value.replace("\r\n", "\n"))
}

pub(super) fn observed(account_id: &str, value: &OofSettings) -> AutoReplySettings {
    let audience = |kind| {
        value.messages.iter().find(|item| item.audience == kind).map(|item| AutoReplyMessage {
            enabled: item.enabled,
            message: item.message.as_deref().map(|text| clean_message(text, item.is_html)),
        })
    };
    AutoReplySettings {
        account_id: account_id.into(),
        state: match value.state {
            OofState::Disabled => AutoReplyState::Disabled,
            OofState::Enabled => AutoReplyState::Enabled,
            OofState::Scheduled => AutoReplyState::Scheduled,
        },
        starts_at: value.starts_at,
        ends_at: value.ends_at,
        internal: audience(OofAudience::Internal),
        external_known: audience(OofAudience::ExternalKnown),
        external_unknown: audience(OofAudience::ExternalUnknown),
        untrusted_external_content: true,
    }
}

pub(super) fn matches(requested: &OofSettings, observed: &OofSettings) -> bool {
    requested.state == observed.state
        && (requested.state != OofState::Scheduled
            || (requested.starts_at == observed.starts_at && requested.ends_at == observed.ends_at))
        && requested.messages.iter().all(|request| {
            observed.messages.iter().find(|item| item.audience == request.audience).is_some_and(
                |actual| {
                    request.enabled == actual.enabled
                        && (!request.enabled
                            || request.message.as_deref().map(|value| value.replace("\r\n", "\n"))
                                == actual
                                    .message
                                    .as_deref()
                                    .map(|value| clean_message(value, actual.is_html)))
                },
            )
        })
}

pub(super) fn preview(input: &AutoReplySetInput, current: &OofSettings) -> WritePreview {
    let mut preview = WritePreview::new("mail_set_auto_reply", input.account_id.clone())
        .field("Current state", format!("{:?}", current.state))
        .field(
            "Current start UTC",
            current.starts_at.map_or_else(String::new, |date| date.to_rfc3339()),
        )
        .field(
            "Current end UTC",
            current.ends_at.map_or_else(String::new, |date| date.to_rfc3339()),
        )
        .field("New state", format!("{:?}", input.state))
        .field("Start UTC", input.starts_at.map_or_else(String::new, |date| date.to_rfc3339()))
        .field("End UTC", input.ends_at.map_or_else(String::new, |date| date.to_rfc3339()))
        .field("Internal message", input.internal_message.as_deref().unwrap_or("preserved"))
        .field("External audience", format!("{:?}", input.external_audience))
        .field("External message", input.external_message.as_deref().unwrap_or("preserved"));
    for (audience, label) in [
        (OofAudience::Internal, "Current internal reply"),
        (OofAudience::ExternalKnown, "Current external contact reply"),
        (OofAudience::ExternalUnknown, "Current other external reply"),
    ] {
        let message = current.messages.iter().find(|item| item.audience == audience);
        let value = message.map_or_else(
            || "not returned".into(),
            |item| {
                format!(
                    "enabled: {}; message: {}",
                    item.enabled,
                    item.message.as_deref().unwrap_or("not set")
                )
            },
        );
        preview = preview.field(label, value);
    }
    preview
}

pub(super) fn existing(record: JournalRecord) -> AutoReplyOperationResult {
    let status = match record.status {
        OperationStatus::Succeeded => AutoReplyOperationState::Succeeded,
        OperationStatus::Partial => AutoReplyOperationState::Partial,
        OperationStatus::Failed => AutoReplyOperationState::Failed,
        OperationStatus::Pending | OperationStatus::Unknown => AutoReplyOperationState::Unknown,
    };
    AutoReplyOperationResult {
        operation_id: record.operation_id,
        status,
        message:
            "the historical automatic-reply outcome is returned without sending another update"
                .into(),
        settings: None,
    }
}

fn clean_message(value: &str, is_html: bool) -> String {
    let text = if is_html { crate::sanitize::plain_text(value) } else { value.to_owned() };
    text.replace("\r\n", "\n")
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .collect()
}

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}
