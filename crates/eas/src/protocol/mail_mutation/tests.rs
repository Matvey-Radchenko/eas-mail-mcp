use super::*;

#[test]
fn property_changes_are_minimal_and_preserve_flag_parameters() -> anyhow::Result<()> {
    let mut previous = element("Email", "Flag");
    for name in ["StartDate", "DueDate", "UtcStartDate", "UtcDueDate"] {
        push_text(&mut previous, "Tasks", name, "2026-10-01T10:00:00.000Z");
    }
    push_text(&mut previous, "Tasks", "ReminderSet", "1");
    push_text(&mut previous, "Email", "Status", "2");
    let request = build_mail_change(
        "f",
        "m",
        "key",
        &MailPatch::Flag { status: 1, previous: Some(previous), updated_at: chrono::Utc::now() },
    )?;
    let tree = decode(&request)?.ok_or_else(|| anyhow::anyhow!("missing request"))?;
    let data = tree
        .descendant("AirSync", "ApplicationData")
        .ok_or_else(|| anyhow::anyhow!("missing data"))?;
    assert_eq!(data.children().count(), 1);
    assert_eq!(
        tree.descendant("AirSync", "GetChanges").map(Element::text_content).as_deref(),
        Some("0")
    );
    assert_eq!(
        data.descendant("Tasks", "UtcDueDate").map(Element::text_content).as_deref(),
        Some("2026-10-01T10:00:00.000Z")
    );
    assert_eq!(data.descendant("Email", "Status").map(Element::text_content).as_deref(), Some("1"));
    assert!(flag::build(0, None, chrono::Utc::now())?.content.is_empty());
    let empty = patch_element(&MailPatch::Categories(Vec::new()))?;
    assert_eq!(empty.name, "Categories");
    assert!(empty.content.is_empty());
    Ok(())
}

#[test]
fn move_requires_a_matching_source_and_confirmed_destination() -> anyhow::Result<()> {
    let mut root = element("Move", "MoveItems");
    let mut response = element("Move", "Response");
    push_text(&mut response, "Move", "SrcMsgId", "old");
    push_text(&mut response, "Move", "Status", "3");
    root.push(response.clone());
    assert!(parse_move(&encode(&root)?, "old").is_err());
    root.content.clear();
    push_text(&mut response, "Move", "DstMsgId", "new");
    root.push(response);
    let parsed = parse_move(&encode(&root)?, "old")?;
    assert_eq!(parsed.status, 3);
    assert_eq!(parsed.server_id.as_deref(), Some("new"));
    assert!(parse_move(&encode(&root)?, "other").is_err());
    Ok(())
}

#[test]
fn sync_success_requires_collection_confirmation_but_not_an_individual_response()
-> anyhow::Result<()> {
    let mut root = element("AirSync", "Sync");
    let mut collection = element("AirSync", "Collection");
    push_text(&mut collection, "AirSync", "CollectionId", "f");
    push_text(&mut collection, "AirSync", "SyncKey", "new");
    let mut collections = element("AirSync", "Collections");
    collections.push(collection.clone());
    root.push(collections);
    assert!(parse_mail_change(&encode(&root)?, "f", "m").is_err());
    root.content.clear();
    push_text(&mut collection, "AirSync", "Status", "1");
    let mut collections = element("AirSync", "Collections");
    collections.push(collection);
    root.push(collections);
    assert_eq!(parse_mail_change(&encode(&root)?, "f", "m")?.status, 1);
    assert!(parse_mail_change(&encode(&root)?, "wrong", "m").is_err());
    let bytes = encode(&root)?;
    let truncated = bytes.get(..bytes.len() - 1).ok_or_else(|| anyhow::anyhow!("missing bytes"))?;
    assert!(parse_mail_change(truncated, "f", "m").is_err());
    Ok(())
}

#[test]
fn conflicting_mutation_acknowledgements_are_never_confirmed() -> anyhow::Result<()> {
    let mut root = element("AirSync", "Sync");
    let mut collection = element("AirSync", "Collection");
    push_text(&mut collection, "AirSync", "CollectionId", "f");
    push_text(&mut collection, "AirSync", "Status", "1");
    push_text(&mut collection, "AirSync", "SyncKey", "new");
    let mut responses = element("AirSync", "Responses");
    for status in ["6", "1"] {
        let mut change = element("AirSync", "Change");
        push_text(&mut change, "AirSync", "ServerId", "m");
        push_text(&mut change, "AirSync", "Status", status);
        responses.push(change);
    }
    collection.push(responses);
    let mut collections = element("AirSync", "Collections");
    collections.push(collection);
    root.push(collections);
    assert!(parse_mail_change(&encode(&root)?, "f", "m").is_err());

    let mut root = element("Move", "MoveItems");
    let mut response = element("Move", "Response");
    push_text(&mut response, "Move", "SrcMsgId", "m");
    push_text(&mut response, "Move", "Status", "3");
    push_text(&mut response, "Move", "Status", "5");
    push_text(&mut response, "Move", "DstMsgId", "new");
    root.push(response);
    assert!(parse_move(&encode(&root)?, "m").is_err());
    Ok(())
}

#[test]
fn change_acknowledgements_require_a_known_unambiguous_sync_status() -> anyhow::Result<()> {
    for status in [1, 3, 4, 5, 6, 7, 8, 9, 12, 13, 15, 16] {
        let parsed = parse_mail_change(&change_reply(status)?, "f", "m")?;
        assert_eq!(parsed.status, status);
        assert_eq!(parsed.sync_key.as_deref(), Some("new"));
    }
    // 14 says the request was processed, and common codes need explicit semantics.
    for status in [0, 2, 10, 11, 14, 17, 100, 101, 110, 153, 154, 999, u16::MAX] {
        assert!(
            matches!(
                parse_mail_change(&change_reply(status)?, "f", "m"),
                Err(EasError::Protocol(_))
            ),
            "unexpectedly confirmed Sync status {status}"
        );
    }
    Ok(())
}

#[test]
fn move_acknowledgements_require_a_known_move_status() -> anyhow::Result<()> {
    for status in [1, 2, 3, 4, 5, 7] {
        assert_eq!(parse_move(&move_reply(status)?, "m")?.status, status);
    }
    for status in [0, 6, 8, 9, 100, 101, 110, 153, 154, 999, u16::MAX] {
        assert!(
            matches!(parse_move(&move_reply(status)?, "m"), Err(EasError::Protocol(_))),
            "unexpectedly confirmed MoveItems status {status}"
        );
    }
    Ok(())
}

fn change_reply(status: u16) -> Result<Vec<u8>> {
    let mut root = element("AirSync", "Sync");
    let mut collection = element("AirSync", "Collection");
    push_text(&mut collection, "AirSync", "CollectionId", "f");
    push_text(&mut collection, "AirSync", "Status", "1");
    push_text(&mut collection, "AirSync", "SyncKey", "new");
    let mut responses = element("AirSync", "Responses");
    let mut change = element("AirSync", "Change");
    push_text(&mut change, "AirSync", "ServerId", "m");
    push_text(&mut change, "AirSync", "Status", status.to_string());
    responses.push(change);
    collection.push(responses);
    let mut collections = element("AirSync", "Collections");
    collections.push(collection);
    root.push(collections);
    encode(&root)
}

fn move_reply(status: u16) -> Result<Vec<u8>> {
    let mut root = element("Move", "MoveItems");
    let mut response = element("Move", "Response");
    push_text(&mut response, "Move", "SrcMsgId", "m");
    push_text(&mut response, "Move", "Status", status.to_string());
    push_text(&mut response, "Move", "DstMsgId", "new");
    root.push(response);
    encode(&root)
}
