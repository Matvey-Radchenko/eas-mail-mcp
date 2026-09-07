use crate::wbxml::{decode, encode};
use crate::{EasError, MeetingResponseChoice, MeetingResponseResult, Result};

use super::mutation_response::{child, malformed, status, text};
use super::tree::{element, push_text};

/// Builds one EAS MeetingResponse request.
pub fn build_meeting_response(
    collection_id: &str,
    request_id: &str,
    response: MeetingResponseChoice,
) -> Result<Vec<u8>> {
    build_meeting_response_instance(collection_id, request_id, response, None)
}

/// Builds a response to a master or its original Calendar occurrence.
pub fn build_meeting_response_instance(
    collection_id: &str,
    request_id: &str,
    response: MeetingResponseChoice,
    original: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<Vec<u8>> {
    if collection_id.is_empty() || request_id.is_empty() {
        return Err(EasError::InvalidConfiguration(
            "MeetingResponse requires collection and request identifiers".into(),
        ));
    }
    let mut root = element("MeetingResponse", "MeetingResponse");
    let mut request = element("MeetingResponse", "Request");
    push_text(&mut request, "MeetingResponse", "UserResponse", response.code().to_string());
    push_text(&mut request, "MeetingResponse", "CollectionId", collection_id);
    push_text(&mut request, "MeetingResponse", "RequestId", request_id);
    if let Some(original) = original {
        push_text(
            &mut request,
            "MeetingResponse",
            "InstanceId",
            original.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        );
    }
    root.push(request);
    encode(&root)
}

/// Builds one EAS MeetingResponse request for a mailbox Search result.
pub fn build_meeting_response_long_id(
    long_id: &str,
    response: MeetingResponseChoice,
) -> Result<Vec<u8>> {
    if long_id.is_empty() || long_id.len() > 256 {
        return Err(EasError::InvalidConfiguration(
            "MeetingResponse LongId must contain 1-256 bytes".into(),
        ));
    }
    let mut root = element("MeetingResponse", "MeetingResponse");
    let mut request = element("MeetingResponse", "Request");
    push_text(&mut request, "MeetingResponse", "UserResponse", response.code().to_string());
    push_text(&mut request, "Search", "LongId", long_id);
    root.push(request);
    encode(&root)
}

/// Parses one EAS MeetingResponse result.
pub fn parse_meeting_response(data: &[u8]) -> Result<MeetingResponseResult> {
    parse_response(data, None)
}

pub(crate) fn parse_for(
    data: &[u8],
    namespace: &str,
    identifier: &str,
) -> Result<MeetingResponseResult> {
    parse_response(data, Some((namespace, identifier)))
}

fn parse_response(data: &[u8], expected: Option<(&str, &str)>) -> Result<MeetingResponseResult> {
    let root = decode(data)?.ok_or_else(|| malformed("empty MeetingResponse acknowledgement"))?;
    if root.namespace != "MeetingResponse"
        || root.name != "MeetingResponse"
        || root.children().count() != 1
    {
        return Err(malformed("MeetingResponse must acknowledge exactly one result"));
    }
    let result = child(&root, "MeetingResponse", "Result")?
        .ok_or_else(|| malformed("MeetingResponse has no direct Result"))?;
    let status = status(result, "MeetingResponse", &[1, 2, 3, 4])?;
    let request_id = text(result, "MeetingResponse", "RequestId")?;
    let long_id = text(result, "Search", "LongId")?;
    if request_id.is_some() && long_id.is_some() {
        return Err(malformed("MeetingResponse has conflicting identifiers"));
    }
    let calendar_id = text(result, "MeetingResponse", "CalendarId")?;
    if request_id.as_ref().is_some_and(String::is_empty)
        || long_id.as_ref().is_some_and(String::is_empty)
        || calendar_id.as_ref().is_some_and(String::is_empty)
    {
        return Err(malformed("MeetingResponse contains an empty identifier"));
    }
    if let Some((namespace, identifier)) = expected {
        let (returned, wrong_kind) = if namespace == "Search" {
            (long_id.as_deref(), request_id.is_some())
        } else {
            (request_id.as_deref(), long_id.is_some())
        };
        if wrong_kind || returned.is_some_and(|returned| returned != identifier) {
            return Err(malformed("MeetingResponse acknowledges a different request"));
        }
    }
    Ok(MeetingResponseResult { status, request_id: request_id.unwrap_or_default(), calendar_id })
}
