use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{Datelike as _, Timelike as _};
use eas_mail_protocol::{CalendarFields, MailFields};
use serde::{Deserialize, Serialize};

use crate::backend::{BackendEvent, BackendMail, MailSource};
use crate::{AppError, ErrorCode, Result};

const PREFIX: &str = "ref1";
const MAX_REFERENCE_BYTES: usize = 48 * 1024;
const MAX_PAYLOAD_BYTES: usize = 32 * 1024;
const MAX_ACCOUNT_BYTES: usize = 256;
const MAX_LOCATOR_BYTES: usize = 8 * 1024;
const MAX_FILENAME_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttachmentReference {
    pub(crate) account_id: String,
    pub(crate) file_reference: String,
    pub(crate) display_name: String,
}

#[derive(Clone)]
pub(crate) enum MeetingReference {
    Event(BackendEvent),
    Mail(BackendMail),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MailPayload {
    account_id: String,
    source: MailLocator,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum MailLocator {
    Item { folder_id: String, server_id: String },
    LongId { long_id: String },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    occurrence_start: Option<chrono::DateTime<chrono::Utc>>,
    account_id: String,
    long_id: String,
    collection_id: Option<String>,
    server_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttachmentPayload {
    account_id: String,
    file_reference: String,
    display_name: String,
}

pub(super) fn encode_mail(value: BackendMail) -> Result<String> {
    let source = match value.source {
        MailSource::Item { folder_id, server_id } => MailLocator::Item { folder_id, server_id },
        MailSource::LongId(long_id) => MailLocator::LongId { long_id },
    };
    let payload = MailPayload { account_id: value.account_id, source };
    validate_mail(&payload)?;
    encode("mail", &payload)
}

pub(super) fn decode_mail(value: &str) -> Result<BackendMail> {
    let payload: MailPayload = decode("mail", value)?;
    validate_mail(&payload)?;
    let (folder_id, source) = match payload.source {
        MailLocator::Item { folder_id, server_id } => {
            let source = MailSource::Item { folder_id: folder_id.clone(), server_id };
            (folder_id, source)
        }
        MailLocator::LongId { long_id } => (String::new(), MailSource::LongId(long_id)),
    };
    Ok(BackendMail {
        account_id: payload.account_id,
        folder_id,
        source,
        fields: MailFields::default(),
    })
}

pub(super) fn encode_event(value: BackendEvent) -> Result<String> {
    let payload = EventPayload {
        occurrence_start: value.occurrence_start,
        account_id: value.account_id,
        long_id: value.long_id,
        collection_id: value.collection_id,
        server_id: value.server_id,
    };
    validate_event(&payload)?;
    encode("event", &payload)
}

pub(super) fn decode_event(value: &str) -> Result<BackendEvent> {
    let payload: EventPayload = decode("event", value)?;
    validate_event(&payload)?;
    Ok(BackendEvent {
        occurrence_start: payload.occurrence_start,
        account_id: payload.account_id,
        long_id: payload.long_id,
        collection_id: payload.collection_id,
        server_id: payload.server_id,
        fields: CalendarFields::default(),
    })
}

pub(super) fn encode_attachment(value: AttachmentReference) -> Result<String> {
    let payload = AttachmentPayload {
        account_id: value.account_id,
        file_reference: value.file_reference,
        display_name: value.display_name,
    };
    validate_attachment(&payload)?;
    encode("attachment", &payload)
}

pub(super) fn decode_attachment(value: &str) -> Result<AttachmentReference> {
    let payload: AttachmentPayload = decode("attachment", value)?;
    validate_attachment(&payload)?;
    Ok(AttachmentReference {
        account_id: payload.account_id,
        file_reference: payload.file_reference,
        display_name: payload.display_name,
    })
}

pub(super) fn decode_meeting(value: &str) -> Result<MeetingReference> {
    match kind(value)? {
        "event" => decode_event(value).map(MeetingReference::Event),
        "mail" => decode_mail(value).map(MeetingReference::Mail),
        _ => Err(invalid()),
    }
}

fn encode(kind: &str, payload: &impl Serialize) -> Result<String> {
    let bytes = serde_json::to_vec(payload).map_err(|_| invalid())?;
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(invalid());
    }
    Ok(format!("{PREFIX}.{kind}.{}", URL_SAFE_NO_PAD.encode(bytes)))
}

fn decode<T: for<'de> Deserialize<'de>>(expected_kind: &str, value: &str) -> Result<T> {
    if value.len() > MAX_REFERENCE_BYTES || kind(value)? != expected_kind {
        return Err(invalid());
    }
    let encoded = value.split_once('.').and_then(|(_, rest)| rest.split_once('.'));
    let (_, encoded) = encoded.ok_or_else(invalid)?;
    let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| invalid())?;
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(invalid());
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}

fn kind(value: &str) -> Result<&str> {
    let mut parts = value.split('.');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(PREFIX), Some(kind), Some(encoded), None)
            if !kind.is_empty() && !encoded.is_empty() =>
        {
            Ok(kind)
        }
        _ => Err(invalid()),
    }
}

fn validate_mail(payload: &MailPayload) -> Result<()> {
    validate_required(&payload.account_id, MAX_ACCOUNT_BYTES)?;
    match &payload.source {
        MailLocator::Item { folder_id, server_id } => {
            validate_required(folder_id, MAX_LOCATOR_BYTES)?;
            validate_required(server_id, MAX_LOCATOR_BYTES)
        }
        MailLocator::LongId { long_id } => validate_required(long_id, MAX_LOCATOR_BYTES),
    }
}

fn validate_event(payload: &EventPayload) -> Result<()> {
    if payload
        .occurrence_start
        .is_some_and(|value| !(1..=9999).contains(&value.year()) || value.nanosecond() != 0)
    {
        return Err(invalid());
    }
    validate_required(&payload.account_id, MAX_ACCOUNT_BYTES)?;
    validate_optional(&payload.long_id, MAX_LOCATOR_BYTES)?;
    if let Some(value) = &payload.collection_id {
        validate_required(value, MAX_LOCATOR_BYTES)?;
    }
    if let Some(value) = &payload.server_id {
        validate_required(value, MAX_LOCATOR_BYTES)?;
    }
    if payload.long_id.is_empty()
        && (payload.collection_id.is_none() || payload.server_id.is_none())
    {
        return Err(invalid());
    }
    Ok(())
}

fn validate_attachment(payload: &AttachmentPayload) -> Result<()> {
    validate_required(&payload.account_id, MAX_ACCOUNT_BYTES)?;
    validate_required(&payload.file_reference, MAX_LOCATOR_BYTES)?;
    validate_required(&payload.display_name, MAX_FILENAME_BYTES)
}

fn validate_required(value: &str, maximum: usize) -> Result<()> {
    if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        return Err(invalid());
    }
    Ok(())
}

fn validate_optional(value: &str, maximum: usize) -> Result<()> {
    if value.len() > maximum || value.chars().any(char::is_control) {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> AppError {
    AppError::new(
        ErrorCode::ValidationFailed,
        "object reference is invalid or unsupported; run list or search again",
    )
}

#[cfg(test)]
mod tests;
