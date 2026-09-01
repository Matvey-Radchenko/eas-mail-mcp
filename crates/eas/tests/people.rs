use eas_mail_protocol::protocol::{build_people_search, parse_people_search};
use eas_mail_protocol::wbxml::{Element, decode, encode};

#[test]
fn gal_search_is_targeted_and_bounded() -> anyhow::Result<()> {
    let tree = decode(&build_people_search("Alice", 20)?)?
        .ok_or_else(|| anyhow::anyhow!("missing request"))?;
    assert_eq!(tree.descendant("Search", "Name").map(Element::text_content), Some("GAL".into()));
    assert_eq!(tree.descendant("Search", "Query").map(Element::text_content), Some("Alice".into()));
    assert_eq!(tree.descendant("Search", "Range").map(Element::text_content), Some("0-19".into()));
    for (query, limit) in [("", 20), ("  ", 20), ("Alice", 0), ("Alice", 51), ("A\nB", 20)] {
        assert!(build_people_search(query, limit).is_err());
    }
    Ok(())
}

#[test]
fn gal_result_drops_everything_except_name_and_email() -> anyhow::Result<()> {
    let mut root = Element::new("Search", "Search");
    root.push(Element::text("Search", "Status", "1"));
    let mut response = Element::new("Search", "Response");
    let mut store = Element::new("Search", "Store");
    store.push(Element::text("Search", "Status", "1"));
    store.push(Element::text("Search", "Total", "3"));
    let mut result = Element::new("Search", "Result");
    let mut properties = Element::new("Search", "Properties");
    properties.push(Element::text("GAL", "DisplayName", "Alice"));
    properties.push(Element::text("GAL", "EmailAddress", "alice@example.invalid"));
    properties.push(Element::text("GAL", "Phone", "not exposed"));
    result.push(properties);
    store.push(result);
    response.push(store);
    root.push(response);
    let result = parse_people_search(&encode(&root)?, 1)?;
    assert_eq!(result.total, 3);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items.first().map(|value| value.name.as_str()), Some("Alice"));
    assert_eq!(
        result.items.first().map(|value| value.email.as_str()),
        Some("alice@example.invalid")
    );
    assert!(parse_people_search(&[0, 1, 2], 1).is_err());
    Ok(())
}
