use crate::wbxml::{decode, encode};
use crate::{EasError, MeetingResponseChoice, MeetingResponseResult, Result};

use super::tree::{direct_text, element, integer, push_text};

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
    let root = decode(data)?
        .ok_or_else(|| EasError::Protocol("Exchange returned an empty MeetingResponse".into()))?;
    let result = root
        .descendant("MeetingResponse", "Result")
        .ok_or_else(|| EasError::Protocol("MeetingResponse has no Result".into()))?;
    Ok(MeetingResponseResult {
        status: integer(direct_text(result, "MeetingResponse", "Status"), 0),
        request_id: direct_text(result, "MeetingResponse", "RequestId").unwrap_or_default(),
        calendar_id: direct_text(result, "MeetingResponse", "CalendarId")
            .filter(|value| !value.is_empty()),
    })
}
