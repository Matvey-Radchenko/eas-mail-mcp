use eas_mail_protocol::EasError;
use eas_mail_protocol::protocol::{
    ComposeSource, build_mime_message, build_send, build_smart, parse_compose,
};
use eas_mail_protocol::wbxml::{Element, decode, encode};

#[test]
fn mime_emits_optional_headers_and_requires_recipient() -> eas_mail_protocol::Result<()> {
    let mime = build_mime_message(
        "sender@example.com",
        &[],
        &["cc@example.com".into()],
        &["bcc@example.com".into()],
        "Subject",
        "Body",
    )?;
    let text = String::from_utf8(mime).map_err(|_| EasError::Protocol("invalid UTF-8".into()))?;
    assert!(text.contains("To: \r\nCc: cc@example.com\r\nBcc: bcc@example.com"));
    assert!(build_mime_message("sender", &[], &[], &[], "subject", "body").is_err());
    for bad in ["bad\rvalue", "bad\nvalue"] {
        assert!(build_mime_message(bad, &["to".into()], &[], &[], "subject", "body").is_err());
        assert!(build_mime_message("from", &[bad.into()], &[], &[], "subject", "body").is_err());
    }
    Ok(())
}

#[test]
fn compose_builders_preserve_opaque_mime_and_sources() -> eas_mail_protocol::Result<()> {
    let send = root(&build_send("client-1", vec![0, 1, 255])?)?;
    assert_eq!(text(&send, "ComposeMail", "ClientId"), Some("client-1".into()));
    assert_eq!(
        send.descendant("ComposeMail", "Mime").and_then(Element::opaque_content),
        Some([0, 1, 255].as_slice())
    );

    let reply =
        root(&build_smart(false, "client-2", ComposeSource::LongId("long-1"), b"reply".to_vec())?)?;
    assert_eq!(reply.name, "SmartReply");
    assert_eq!(text(&reply, "ComposeMail", "LongId"), Some("long-1".into()));

    let forward = root(&build_smart(
        true,
        "client-3",
        ComposeSource::Item { folder_id: "inbox", item_id: "item-1" },
        b"forward".to_vec(),
    )?)?;
    assert_eq!(forward.name, "SmartForward");
    assert_eq!(text(&forward, "ComposeMail", "FolderId"), Some("inbox".into()));
    assert_eq!(text(&forward, "ComposeMail", "ItemId"), Some("item-1".into()));

    assert!(build_smart(false, "client", ComposeSource::LongId(""), Vec::new()).is_err());
    assert!(
        build_smart(
            true,
            "client",
            ComposeSource::Item { folder_id: "", item_id: "item" },
            Vec::new(),
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn compose_parser_accepts_empty_success_and_reads_status() -> eas_mail_protocol::Result<()> {
    assert_eq!(parse_compose(&[])?.status, 1);
    let mut root = Element::new("ComposeMail", "SendMail");
    root.push(Element::text("ComposeMail", "Status", "122"));
    assert_eq!(parse_compose(&encode(&root)?)?.status, 122);
    assert!(parse_compose(&[0xFF]).is_err());
    Ok(())
}

fn root(data: &[u8]) -> eas_mail_protocol::Result<Element> {
    decode(data)?.ok_or_else(|| EasError::Protocol("expected WBXML document".into()))
}

fn text(root: &Element, namespace: &str, name: &str) -> Option<String> {
    root.descendant(namespace, name).map(Element::text_content)
}
