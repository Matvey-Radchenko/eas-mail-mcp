use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use eas_mail_protocol::protocol::{
    ComposeSource, MAX_ATTACHMENT_BYTES, MAX_MIME_BYTES, MimeAttachment, MimeMessage,
    build_mime_message, build_mime_with_attachments, build_smart,
};
use eas_mail_protocol::wbxml::decode;

fn message(to: &[String]) -> MimeMessage<'_> {
    MimeMessage {
        sender: "from@example.invalid",
        to,
        cc: &[],
        bcc: &[],
        subject: "Отчёт",
        body: "Body",
    }
}

fn attachment() -> MimeAttachment {
    MimeAttachment {
        filename: "отчёт.bin".into(),
        content_type: "application/octet-stream".into(),
        bytes: vec![0, 255, 10, 13, 1, 254],
    }
}

#[test]
fn multipart_preserves_binary_bytes_and_encodes_unicode_filename() -> anyhow::Result<()> {
    let recipients = vec!["to@example.invalid".into()];
    let attachment = attachment();
    let mime =
        build_mime_with_attachments(message(&recipients), std::slice::from_ref(&attachment))?;
    let text = String::from_utf8(mime.clone())?;
    assert!(text.contains("Content-Type: multipart/mixed"));
    assert!(text.contains("Content-Disposition: attachment"));
    let (_, filename) = text
        .split_once("filename=\"=?utf-8?B?")
        .ok_or_else(|| anyhow::anyhow!("missing encoded UTF-8 filename"))?;
    let filename = filename
        .split("?=\"")
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing encoded filename terminator"))?;
    assert_eq!(String::from_utf8(STANDARD.decode(filename)?)?, attachment.filename);
    let (_, encoded) = text
        .split_once("Content-Transfer-Encoding: base64\r\n\r\n")
        .ok_or_else(|| anyhow::anyhow!("missing base64 attachment"))?;
    let encoded = encoded
        .split("\r\n--")
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing boundary"))?
        .split_whitespace()
        .collect::<String>();
    assert_eq!(STANDARD.decode(encoded)?, attachment.bytes);
    for forward in [false, true] {
        let request =
            build_smart(forward, "client", ComposeSource::LongId("source"), mime.clone())?;
        let root = decode(&request)?.ok_or_else(|| anyhow::anyhow!("missing compose"))?;
        assert!(root.descendant("ComposeMail", "ReplaceMime").is_none());
        assert_eq!(
            root.descendant("ComposeMail", "Mime").and_then(|e| e.opaque_content()),
            Some(mime.as_slice())
        );
    }
    Ok(())
}

#[test]
fn no_attachment_wire_form_is_unchanged_and_metadata_is_strict() -> anyhow::Result<()> {
    let recipients = vec!["to@example.invalid".into()];
    let fields = message(&recipients);
    let legacy = build_mime_message(
        fields.sender,
        fields.to,
        fields.cc,
        fields.bcc,
        fields.subject,
        fields.body,
    )?;
    assert_eq!(build_mime_with_attachments(fields, &[])?, legacy);
    for name in ["../file", "folder\\file", "bad\nheader", "", "."] {
        assert!(MimeAttachment::validate_metadata(name, "application/pdf").is_err());
    }
    for kind in ["text", "text/", "text/plain; charset=utf8", "text/plain\r\n", "a/b/c"] {
        assert!(MimeAttachment::validate_metadata("file", kind).is_err());
    }
    Ok(())
}

#[test]
fn raw_and_encoded_size_limits_are_enforced() -> anyhow::Result<()> {
    let recipients = vec!["to@example.invalid".into()];
    assert!(build_mime_with_attachments(message(&recipients), &vec![attachment(); 21]).is_err());
    let mut large = attachment();
    large.bytes.resize(MAX_ATTACHMENT_BYTES + 1, 0);
    assert!(build_mime_with_attachments(message(&recipients), &[large]).is_err());
    let body = "a".repeat(MAX_MIME_BYTES);
    let mut fields = message(&recipients);
    fields.body = &body;
    assert!(build_mime_with_attachments(fields, &[attachment()]).is_err());
    Ok(())
}
