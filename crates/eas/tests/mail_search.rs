use eas_mail_protocol::protocol::{build_mail_search, parse_item_fetch, parse_mail_search};
use eas_mail_protocol::wbxml::{Element, Node, decode, encode};
use eas_mail_protocol::{MailSearchQuery, Patch};

#[test]
fn documented_predicates_and_conversation_are_direct_children_of_and() -> anyhow::Result<()> {
    let query = MailSearchQuery {
        text: String::new(),
        folder_ids: vec!["inbox".into(), "sent".into()],
        received_after: Some(chrono::DateTime::UNIX_EPOCH),
        received_before: Some(chrono::DateTime::UNIX_EPOCH + chrono::Duration::days(1)),
        conversation_id: Some(vec![0x80; 16]),
    };
    let tree = decode(&build_mail_search(&query, 0, 100, 500)?)?
        .ok_or_else(|| anyhow::anyhow!("empty request"))?;
    let and = tree.descendant("Search", "And").ok_or_else(|| anyhow::anyhow!("missing And"))?;
    assert!(and.child("Search", "ConversationId").is_some());
    assert_eq!(
        and.child("Search", "ConversationId").map(|value| &value.content),
        Some(&vec![Node::Opaque(vec![0x80; 16])])
    );
    assert!(and.child("Search", "FreeText").is_none());
    assert_eq!(and.descendants("AirSync", "CollectionId").len(), 2);
    let greater =
        and.child("Search", "GreaterThan").ok_or_else(|| anyhow::anyhow!("missing date"))?;
    assert_eq!(greater.children().next().map(|value| value.name.as_str()), Some("DateReceived"));
    assert!(tree.descendant("Search", "DeepTraversal").is_none());
    assert!(tree.descendant("Search", "RebuildResults").is_some());
    assert!(build_mail_search(&query, 1000, 1, 0).is_err());
    Ok(())
}

#[test]
fn search_checks_store_status_and_preserves_optional_coverage() -> anyhow::Result<()> {
    let limited = parse_mail_search(&encode(&response("12", true))?)?;
    assert!(limited.server_truncated);
    assert_eq!(limited.items.len(), 1);
    assert!(parse_mail_search(&encode(&response("8", false))?).is_err());
    let page = parse_mail_search(&encode(&response("1", false))?)?;
    assert_eq!(page.total, None);
    assert_eq!(page.range, None);
    let page = parse_mail_search(&encode(&response("1", true))?)?;
    assert_eq!(page.total, Some(123));
    let item = page.items.first().ok_or_else(|| anyhow::anyhow!("missing candidate"))?;
    assert_eq!(item.collection_id.as_deref(), Some("inbox"));
    assert_eq!(item.server_id.as_deref(), Some("message-1"));
    assert_eq!(item.fields.conversation_id, Patch::Value(vec![0xff; 16]));
    assert_eq!(item.fields.is_read, Patch::Missing);
    Ok(())
}

#[test]
fn item_fetch_preserves_mutable_locator_and_binary_metadata() -> anyhow::Result<()> {
    let mut root = Element::new("ItemOperations", "ItemOperations");
    let mut response = Element::new("ItemOperations", "Response");
    let mut fetch = Element::new("ItemOperations", "Fetch");
    fetch.push(Element::text("ItemOperations", "Status", "1"));
    fetch.push(Element::text("AirSync", "CollectionId", "inbox"));
    fetch.push(Element::text("AirSync", "ServerId", "message-1"));
    let mut properties = properties();
    properties.namespace = "ItemOperations".into();
    fetch.push(properties);
    response.push(fetch);
    root.push(response);
    let item = parse_item_fetch(&encode(&root)?)?;
    assert_eq!(item.collection_id.as_deref(), Some("inbox"));
    assert_eq!(item.server_id.as_deref(), Some("message-1"));
    assert_eq!(item.fields.conversation_id, Patch::Value(vec![0xff; 16]));
    Ok(())
}

#[test]
fn empty_result_sentinel_is_not_a_malformed_message() -> anyhow::Result<()> {
    let mut root = Element::new("Search", "Search");
    root.push(Element::text("Search", "Status", "1"));
    let mut response = Element::new("Search", "Response");
    let mut store = Element::new("Search", "Store");
    store.push(Element::text("Search", "Status", "1"));
    store.push(Element::new("Search", "Result"));
    response.push(store);
    root.push(response);
    let page = parse_mail_search(&encode(&root)?)?;
    assert!(page.items.is_empty());
    assert_eq!(page.total, None);
    Ok(())
}

fn response(status: &str, metadata: bool) -> Element {
    let mut root = Element::new("Search", "Search");
    root.push(Element::text("Search", "Status", "1"));
    let mut response = Element::new("Search", "Response");
    let mut store = Element::new("Search", "Store");
    store.push(Element::text("Search", "Status", status));
    if metadata {
        store.push(Element::text("Search", "Total", "123"));
        store.push(Element::text("Search", "Range", "0-0"));
    }
    let mut result = Element::new("Search", "Result");
    result.push(Element::text("Search", "LongId", "long-1"));
    result.push(Element::text("AirSync", "CollectionId", "inbox"));
    result.push(Element::text("AirSync", "ServerId", "message-1"));
    result.push(properties());
    store.push(result);
    response.push(store);
    root.push(response);
    root
}

fn properties() -> Element {
    let mut properties = Element::new("Search", "Properties");
    properties.push(Element::text("Email", "Subject", "Synthetic thread"));
    let mut id = Element::new("Email2", "ConversationId");
    id.content.push(Node::Opaque(vec![0xff; 16]));
    properties.push(id);
    properties
}
