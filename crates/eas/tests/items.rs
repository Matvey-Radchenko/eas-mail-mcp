#![expect(
    clippy::indexing_slicing,
    reason = "fixed test fixtures use direct indexing for readable assertions"
)]

use base64::Engine as _;

use eas_mail_protocol::protocol::{
    build_attachment_fetch, build_item_fetch, build_search, parse_attachment_fetch,
    parse_item_fetch, parse_search,
};
use eas_mail_protocol::wbxml::{Element, Node, decode, encode};
use eas_mail_protocol::{EasError, Patch};

#[test]
fn search_request_validates_range_and_preview() -> eas_mail_protocol::Result<()> {
    for invalid in [build_search("", 0, 1, 500), build_search("x", 0, 0, 500)] {
        assert!(matches!(invalid, Err(EasError::InvalidConfiguration(_))));
    }
    assert!(build_search("x", 0, 101, 500).is_err());

    let root = required_root(&build_search("  quarterly report  ", 7, 3, 900)?)?;
    let query = root
        .descendant("Search", "Query")
        .ok_or_else(|| EasError::Protocol("search request has no Query".into()))?;
    let conjunction = query
        .child("Search", "And")
        .ok_or_else(|| EasError::Protocol("search request has no And".into()))?;
    assert_eq!(
        conjunction.child("AirSync", "Class").map(Element::text_content),
        Some("Email".into())
    );
    assert_eq!(
        conjunction.child("Search", "FreeText").map(Element::text_content),
        Some("  quarterly report  ".into())
    );
    assert_eq!(text(&root, "Search", "Range"), Some("7-9".into()));
    assert_eq!(text(&root, "AirSyncBase", "TruncationSize"), Some("500".into()));
    assert!(root.descendant("Search", "DeepTraversal").is_some());
    assert!(build_search("x", 0, 100, 500).is_ok());
    Ok(())
}

#[test]
fn search_parser_handles_empty_errors_and_complete_mail() -> eas_mail_protocol::Result<()> {
    assert!(parse_search(&[]).is_err());
    let error = search_response(7, false, false)?;
    assert!(matches!(parse_search(&error), Err(EasError::Protocol(_))));

    assert!(parse_search(&search_response(1, true, true)?).is_err());
    let response = search_response(1, true, false)?;
    let output = parse_search(&response)?;
    assert_eq!(output.len(), 1);
    let result = &output[0];
    assert_eq!(result.long_id, "long-1");
    assert_eq!(result.fields.subject, Patch::Value("Report".into()));
    assert_eq!(result.fields.body, Patch::Value("Preview".into()));
    assert_eq!(result.fields.body_truncated, Patch::Value(true));
    assert_eq!(result.fields.is_read, Patch::Value(true));
    assert_eq!(result.fields.importance, Patch::Value(2));
    assert_eq!(result.fields.message_class, Patch::Value("IPM.Schedule.Meeting.Request".into()));
    let Patch::Value(meeting) = &result.fields.meeting_request else {
        return Err(EasError::Protocol("meeting request was not parsed".into()));
    };
    assert_eq!(meeting.organizer, "Organizer <organizer@example.com>");
    assert_eq!(meeting.message_type, 1);
    assert!(meeting.response_requested);
    assert_eq!(
        meeting.starts_at.map(|value| value.to_rfc3339()).as_deref(),
        Some("2026-08-24T09:00:00+00:00")
    );
    let Patch::Value(attachments) = &result.fields.attachments else {
        return Err(EasError::Protocol("attachments were not parsed".into()));
    };
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].display_name, "_report_2026?.pdf");
    assert_eq!(attachments[0].size, 42);
    assert!(attachments[0].is_inline);
    Ok(())
}

#[test]
fn item_fetch_builds_both_reference_forms_and_validates_input() -> eas_mail_protocol::Result<()> {
    let long = required_root(&build_item_fetch(Some("long-1"), None, None, 90_000)?)?;
    assert_eq!(text(&long, "Search", "LongId"), Some("long-1".into()));
    assert_eq!(text(&long, "AirSyncBase", "TruncationSize"), Some("50000".into()));

    let item = required_root(&build_item_fetch(None, Some("inbox"), Some("mail-1"), 12_000)?)?;
    assert_eq!(text(&item, "AirSync", "CollectionId"), Some("inbox".into()));
    assert_eq!(text(&item, "AirSync", "ServerId"), Some("mail-1".into()));
    for invalid in [
        build_item_fetch(None, None, None, 1),
        build_item_fetch(Some(""), None, None, 1),
        build_item_fetch(None, Some(""), Some("mail"), 1),
    ] {
        assert!(matches!(invalid, Err(EasError::InvalidConfiguration(_))));
    }
    Ok(())
}

#[test]
fn item_fetch_parser_requires_successful_fetch_and_properties() -> eas_mail_protocol::Result<()> {
    assert!(parse_item_fetch(&[]).is_err());
    assert!(parse_item_fetch(&encode(&Element::new("ItemOperations", "ItemOperations"))?).is_err());
    assert!(parse_item_fetch(&item_response(6, false)?).is_err());
    assert!(parse_item_fetch(&item_response(1, false)?).is_err());

    let result = parse_item_fetch(&item_response(1, true)?)?;
    assert_eq!(result.fields.subject, Patch::Value("Full message".into()));
    assert_eq!(result.fields.body_truncated, Patch::Value(true));
    Ok(())
}

#[test]
fn attachment_fetch_supports_opaque_and_base64_and_rejects_bad_data()
-> eas_mail_protocol::Result<()> {
    assert!(build_attachment_fetch("").is_err());
    let request = required_root(&build_attachment_fetch("file-1")?)?;
    assert_eq!(text(&request, "AirSyncBase", "FileReference"), Some("file-1".into()));

    assert_eq!(parse_attachment_fetch(&attachment_response(1, Data::Opaque)?)?, [0, 1, 255]);
    assert_eq!(parse_attachment_fetch(&attachment_response(1, Data::Base64)?)?, b"payload");
    assert!(parse_attachment_fetch(&[]).is_err());
    assert!(
        parse_attachment_fetch(&encode(&Element::new("ItemOperations", "ItemOperations"))?)
            .is_err()
    );
    assert!(parse_attachment_fetch(&attachment_response(8, Data::Opaque)?).is_err());
    assert!(parse_attachment_fetch(&attachment_response(1, Data::Missing)?).is_err());
    assert!(parse_attachment_fetch(&attachment_response(1, Data::Invalid)?).is_err());
    Ok(())
}

enum Data {
    Opaque,
    Base64,
    Invalid,
    Missing,
}

fn search_response(status: u16, valid: bool, ignored: bool) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("Search", "Search");
    root.push(Element::text("Search", "Status", status.to_string()));
    let mut response = Element::new("Search", "Response");
    let mut store = Element::new("Search", "Store");
    store.push(Element::text("Search", "Status", "1"));
    if valid {
        let mut result = Element::new("Search", "Result");
        result.push(Element::text("Search", "LongId", "long-1"));
        result.push(mail_properties("Report", "Preview"));
        store.push(result);
    }
    if ignored {
        let mut result = Element::new("Search", "Result");
        result.push(mail_properties("Ignored", "Ignored"));
        store.push(result);
    }
    response.push(store);
    root.push(response);
    encode(&root)
}

fn mail_properties(subject: &str, body: &str) -> Element {
    let mut properties = Element::new("Search", "Properties");
    properties.push(Element::text("Email", "Subject", subject));
    properties.push(Element::text("Email", "From", "sender@example.com"));
    properties.push(Element::text("Email", "To", "recipient@example.com"));
    properties.push(Element::text("Email", "Cc", "cc@example.com"));
    properties.push(Element::text("Email", "DateReceived", "20260102T030405Z"));
    properties.push(Element::text("Email", "Read", "1"));
    properties.push(Element::text("Email", "Importance", "2"));
    properties.push(Element::text("Email", "MessageClass", "IPM.Schedule.Meeting.Request"));
    let mut meeting = Element::new("Email", "MeetingRequest");
    meeting.push(Element::text("Email", "AllDayEvent", "0"));
    meeting.push(Element::text("Email", "DtStamp", "20260822T100000Z"));
    meeting.push(Element::text("Email", "StartTime", "20260824T090000Z"));
    meeting.push(Element::text("Email", "EndTime", "20260824T100000Z"));
    meeting.push(Element::text("Email", "InstanceType", "0"));
    meeting.push(Element::text("Email", "Location", "Room 1"));
    meeting.push(Element::text("Email", "Organizer", "Organizer <organizer@example.com>"));
    meeting.push(Element::text("Email", "Reminder", "15"));
    meeting.push(Element::text("Email", "ResponseRequested", "1"));
    meeting.push(Element::text("Email", "BusyStatus", "2"));
    meeting.push(Element::text("Email", "TimeZone", "AAAA"));
    meeting.push(Element::text(
        "Email",
        "GlobalObjId",
        "BAAAAIIA4AB0xbcQGoLgCAAAAAAAAAAAAAAAAAAAAAAAAAAAMwAAAHZDYWwtVWlkAQAAAHs4MTQxMkQzQy0yQTI0LTRFOUQtQjIwRS0xMUY3QkJFOTI3OTl9AA==",
    ));
    meeting.push(Element::text("Email2", "MeetingMessageType", "1"));
    properties.push(meeting);
    let mut body_element = Element::new("AirSyncBase", "Body");
    body_element.push(Element::text("AirSyncBase", "Data", body));
    body_element.push(Element::text("AirSyncBase", "Truncated", "1"));
    properties.push(body_element);
    let mut attachments = Element::new("AirSyncBase", "Attachments");
    let mut attachment = Element::new("AirSyncBase", "Attachment");
    attachment.push(Element::text("AirSyncBase", "DisplayName", "../report:2026?.pdf"));
    attachment.push(Element::text("AirSyncBase", "FileReference", "file-1"));
    attachment.push(Element::text("AirSyncBase", "EstimatedDataSize", "42"));
    attachment.push(Element::text("AirSyncBase", "ContentType", "application/pdf"));
    attachment.push(Element::text("AirSyncBase", "IsInline", "1"));
    attachment.push(Element::text("AirSyncBase", "ContentId", "cid-1"));
    attachments.push(attachment);
    attachments.push(Element::new("AirSyncBase", "Attachment"));
    properties.push(attachments);
    properties
}

fn item_response(status: u16, properties: bool) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("ItemOperations", "ItemOperations");
    let mut fetch = Element::new("ItemOperations", "Fetch");
    fetch.push(Element::text("ItemOperations", "Status", status.to_string()));
    if properties {
        let mut value = mail_properties("Full message", "Full body");
        value.namespace = "ItemOperations".into();
        fetch.push(value);
    }
    root.push(fetch);
    encode(&root)
}

fn attachment_response(status: u16, data: Data) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("ItemOperations", "ItemOperations");
    let mut fetch = Element::new("ItemOperations", "Fetch");
    fetch.push(Element::text("ItemOperations", "Status", status.to_string()));
    match data {
        Data::Opaque => {
            let mut value = Element::new("ItemOperations", "Data");
            value.content.push(Node::Opaque(vec![0, 1, 255]));
            fetch.push(value);
        }
        Data::Base64 => fetch.push(Element::text(
            "AirSyncBase",
            "Data",
            base64::engine::general_purpose::STANDARD.encode(b"payload"),
        )),
        Data::Invalid => fetch.push(Element::text("ItemOperations", "Data", "%%%")),
        Data::Missing => {}
    }
    root.push(fetch);
    encode(&root)
}

fn required_root(data: &[u8]) -> eas_mail_protocol::Result<Element> {
    decode(data)?.ok_or_else(|| EasError::Protocol("expected WBXML document".into()))
}

fn text(root: &Element, namespace: &str, name: &str) -> Option<String> {
    root.descendant(namespace, name).map(Element::text_content)
}
