use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use eas_mail_protocol::{CalendarFields, MailFields};

use super::*;

#[test]
fn every_reference_kind_round_trips_without_content() -> Result<()> {
    let mail = BackendMail {
        account_id: "work".into(),
        folder_id: "inbox".into(),
        source: MailSource::Item { folder_id: "inbox".into(), server_id: "message-1".into() },
        fields: MailFields::default(),
    };
    let encoded_mail = encode_mail(mail.clone())?;
    assert_eq!(decode_mail(&encoded_mail)?, mail);
    assert!(!decoded_json(&encoded_mail)?.contains("subject"));

    let event = BackendEvent {
        occurrence_start: None,
        account_id: "work".into(),
        long_id: "long-1".into(),
        collection_id: Some("calendar".into()),
        server_id: Some("event-1".into()),
        fields: CalendarFields::default(),
    };
    let encoded_event = encode_event(event.clone())?;
    assert_eq!(decode_event(&encoded_event)?, event);

    let attachment = AttachmentReference {
        account_id: "work".into(),
        file_reference: "file-1".into(),
        display_name: "report.txt".into(),
    };
    let encoded_attachment = encode_attachment(attachment.clone())?;
    assert_eq!(decode_attachment(&encoded_attachment)?, attachment);
    Ok(())
}

#[test]
fn long_id_and_meeting_kinds_round_trip() -> Result<()> {
    let mail = BackendMail {
        account_id: "work".into(),
        folder_id: String::new(),
        source: MailSource::LongId("search-long-id".into()),
        fields: MailFields::default(),
    };
    let mail_ref = encode_mail(mail)?;
    assert!(matches!(decode_meeting(&mail_ref)?, MeetingReference::Mail(_)));

    let event = BackendEvent {
        occurrence_start: None,
        account_id: "work".into(),
        long_id: String::new(),
        collection_id: Some("calendar".into()),
        server_id: Some("event-1".into()),
        fields: CalendarFields::default(),
    };
    let event_ref = encode_event(event)?;
    assert!(matches!(decode_meeting(&event_ref)?, MeetingReference::Event(_)));
    Ok(())
}

#[test]
fn malformed_wrong_kind_and_oversized_references_are_rejected() -> Result<()> {
    let malformed = [
        "mail_old-uuid",
        "ref2.mail.e30",
        "ref1.mail.not-base64!",
        "ref1.unknown.e30",
        "ref1.mail.e30.extra",
    ];
    for value in malformed {
        let Err(error) = decode_mail(value) else {
            return Err(AppError::new(
                ErrorCode::ProtocolError,
                "malformed reference was accepted",
            ));
        };
        assert_eq!(error.envelope.code, ErrorCode::ValidationFailed);
    }

    let oversized = format!("ref1.mail.{}", "a".repeat(MAX_REFERENCE_BYTES));
    assert!(decode_mail(&oversized).is_err());
    let attachment = AttachmentReference {
        account_id: "work".into(),
        file_reference: "x".repeat(MAX_LOCATOR_BYTES + 1),
        display_name: "file.txt".into(),
    };
    assert!(encode_attachment(attachment).is_err());
    Ok(())
}

#[test]
fn unknown_json_fields_and_incomplete_event_locators_are_rejected() -> Result<()> {
    let unknown = encoded(
        "mail",
        r#"{"account_id":"work","source":{"kind":"long_id","long_id":"id"},"secret":"no"}"#,
    );
    assert!(decode_mail(&unknown).is_err());

    let incomplete = encoded(
        "event",
        r#"{"account_id":"work","long_id":"","collection_id":"calendar","server_id":null}"#,
    );
    assert!(decode_event(&incomplete).is_err());
    Ok(())
}

#[test]
fn occurrence_references_keep_original_start_and_reject_invalid_timestamps() -> Result<()> {
    let legacy = r#"{"account_id":"work","long_id":"id","collection_id":null,"server_id":null}"#;
    let mut event = decode_event(&encoded("event", legacy))?;
    assert_eq!(event.occurrence_start, None);
    let original = chrono::DateTime::parse_from_rfc3339("2026-03-08T13:00:00Z")
        .map_err(|_| invalid())?
        .with_timezone(&chrono::Utc);
    event.occurrence_start = Some(original);
    let reference = encode_event(event.clone())?;
    assert_eq!(decode_event(&reference)?, event);
    let payload: serde_json::Value =
        serde_json::from_str(&decoded_json(&reference)?).map_err(|_| invalid())?;
    assert_eq!(
        payload.get("occurrence_start").and_then(serde_json::Value::as_str),
        Some("2026-03-08T13:00:00Z")
    );
    for timestamp in ["bad", "2026-03-08T13:00:00.001Z", "+10000-01-01T00:00:00Z"] {
        let mut malformed = payload.clone();
        *malformed.get_mut("occurrence_start").ok_or_else(invalid)? = timestamp.into();
        assert!(decode_event(&encoded("event", &malformed.to_string())).is_err());
    }
    Ok(())
}

#[test]
fn event_references_preserve_uid_and_legacy_references_remain_supported() -> Result<()> {
    let mut event = BackendEvent {
        occurrence_start: None,
        account_id: "work".into(),
        long_id: String::new(),
        collection_id: Some("calendar".into()),
        server_id: Some("event-1".into()),
        fields: CalendarFields { uid: Patch::Value("uid-1".into()), ..CalendarFields::default() },
    };
    let reference = encode_event(event.clone())?;
    assert_eq!(decode_event(&reference)?, event);

    let legacy = encoded(
        "event",
        r#"{"account_id":"work","long_id":"","collection_id":"calendar","server_id":"event-1"}"#,
    );
    event.fields = CalendarFields::default();
    assert_eq!(decode_event(&legacy)?, event);
    Ok(())
}

fn encoded(kind: &str, json: &str) -> String {
    format!("ref1.{kind}.{}", URL_SAFE_NO_PAD.encode(json.as_bytes()))
}

fn decoded_json(value: &str) -> Result<String> {
    let encoded = value.rsplit_once('.').map(|(_, value)| value).ok_or_else(invalid)?;
    let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| invalid())?;
    String::from_utf8(bytes).map_err(|_| invalid())
}
