use chrono::SecondsFormat;

use crate::wbxml::{Element, Node, decode, encode};
use crate::{EasError, MailSearchQuery, Result, SearchMail, SearchMailPage, SearchRange};

use super::sync::parse_mail_fields;
use super::tree::{direct_text, element, push_text};

/// Builds bounded EAS 14.1 mail search using only documented server predicates.
pub fn build_mail_search(
    query: &MailSearchQuery,
    start: usize,
    limit: usize,
    preview_size: usize,
) -> Result<Vec<u8>> {
    if limit == 0 || limit > 100 || start > 999 || start.saturating_add(limit) > 1000 {
        return Err(invalid("mail search range exceeds 100 results or 1000 candidates"));
    }
    let mut root = element("Search", "Search");
    let mut store = element("Search", "Store");
    push_text(&mut store, "Search", "Name", "Mailbox");
    let mut predicate = element("Search", "Query");
    let mut conjunction = element("Search", "And");
    push_text(&mut conjunction, "AirSync", "Class", "Email");
    if !query.text.trim().is_empty() {
        push_text(&mut conjunction, "Search", "FreeText", &query.text);
    }
    // MS-ASCMD 2.2.3.30.5 permits repeated CollectionId directly under And.
    for folder in &query.folder_ids {
        if folder.is_empty() {
            return Err(invalid("mail search folder must not be empty"));
        }
        push_text(&mut conjunction, "AirSync", "CollectionId", folder);
    }
    for (operator, timestamp) in
        [("GreaterThan", query.received_after), ("LessThan", query.received_before)]
    {
        if let Some(timestamp) = timestamp {
            let mut comparison = element("Search", operator);
            comparison.push(element("Email", "DateReceived"));
            push_text(
                &mut comparison,
                "Search",
                "Value",
                timestamp.to_rfc3339_opts(SecondsFormat::Millis, true),
            );
            conjunction.push(comparison);
        }
    }
    if let Some(id) = &query.conversation_id {
        if id.len() != 16 {
            return Err(invalid("conversation identifier must contain 16 opaque bytes"));
        }
        let mut conversation = element("Search", "ConversationId");
        conversation.content.push(Node::Opaque(id.clone()));
        conjunction.push(conversation);
    }
    predicate.push(conjunction);
    store.push(predicate);
    let mut options = element("Search", "Options");
    push_text(&mut options, "Search", "Range", format!("{start}-{}", start + limit - 1));
    if query.folder_ids.is_empty() {
        options.push(element("Search", "DeepTraversal"));
    }
    if start == 0 {
        options.push(element("Search", "RebuildResults"));
    }
    let mut body = element("AirSyncBase", "BodyPreference");
    push_text(&mut body, "AirSyncBase", "Type", "1");
    push_text(&mut body, "AirSyncBase", "TruncationSize", preview_size.min(500).to_string());
    options.push(body);
    store.push(options);
    root.push(store);
    encode(&root)
}

/// Parses both Search status levels, optional coverage and portable item locators.
pub fn parse_mail_search(data: &[u8]) -> Result<SearchMailPage> {
    let root = decode(data)?.ok_or_else(|| protocol("empty Search response"))?;
    search_status(&root)?;
    let store = root
        .child("Search", "Response")
        .and_then(|response| response.child("Search", "Store"))
        .ok_or_else(|| protocol("Search response has no Store"))?;
    let server_truncated = match direct_text(store, "Search", "Status").as_deref() {
        Some("1") => false,
        Some("12") => true,
        _ => return Err(protocol("Search store rejected the request")),
    };
    let total = direct_text(store, "Search", "Total")
        .map(|value| value.parse().map_err(|_| protocol("invalid Search total")))
        .transpose()?;
    let range = direct_text(store, "Search", "Range").map(parse_range).transpose()?;
    let items = parse_results(store)?;
    if items.len() > 100 {
        return Err(protocol("Search returned more than 100 candidates"));
    }
    if let Some(range) = range
        && range.end.saturating_sub(range.start).saturating_add(1) != items.len()
    {
        return Err(protocol("Search range disagrees with returned candidates"));
    }
    Ok(SearchMailPage { items, total, range, server_truncated })
}

fn parse_results(store: &Element) -> Result<Vec<SearchMail>> {
    let results = store
        .children()
        .filter(|value| value.namespace == "Search" && value.name == "Result")
        .collect::<Vec<_>>();
    // Exchange returns one self-closing Result for an empty search. A mixed
    // empty/populated result set is malformed and must not silently lose items.
    if results.len() == 1 && results.first().is_some_and(|value| value.content.is_empty()) {
        return Ok(Vec::new());
    }
    results.into_iter().map(parse_result).collect()
}

pub(super) fn search_status(element: &Element) -> Result<()> {
    match direct_text(element, "Search", "Status").as_deref() {
        Some("1") => Ok(()),
        _ => Err(protocol("Search command or store rejected the request")),
    }
}

fn parse_range(value: String) -> Result<SearchRange> {
    let (start, end) = value.split_once('-').ok_or_else(|| protocol("invalid Search range"))?;
    let start = start.parse::<usize>().map_err(|_| protocol("invalid Search range start"))?;
    let end = end.parse::<usize>().map_err(|_| protocol("invalid Search range end"))?;
    if start > end || end >= 1000 {
        return Err(protocol("Search range is reversed or exceeds the candidate limit"));
    }
    Ok(SearchRange { start, end })
}

fn parse_result(result: &Element) -> Result<SearchMail> {
    let long_id = direct_text(result, "Search", "LongId")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| protocol("Search candidate has no LongId"))?;
    let properties = result
        .child("Search", "Properties")
        .ok_or_else(|| protocol("Search candidate has no Properties"))?;
    Ok(SearchMail {
        long_id,
        collection_id: direct_text(result, "AirSync", "CollectionId").filter(|v| !v.is_empty()),
        server_id: direct_text(result, "AirSync", "ServerId").filter(|v| !v.is_empty()),
        fields: parse_mail_fields(properties),
    })
}

fn invalid(message: &'static str) -> EasError {
    EasError::InvalidConfiguration(message.into())
}

fn protocol(message: &'static str) -> EasError {
    EasError::Protocol(message.into())
}
