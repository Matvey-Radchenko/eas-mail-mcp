use base64::Engine as _;

use crate::wbxml::{decode, encode};
use crate::{
    CalendarItemResult, EasError, ItemResult, Result, SearchCalendar, SearchCalendarPage,
    SearchMail,
};

use super::sync::{parse_calendar_fields, parse_mail_fields};
use super::tree::{descendant_text, direct_text, element, integer, push_text};

/// Builds a server-side mailbox Search request with a 500-character plain preview.
pub fn build_search(
    query: &str,
    start: usize,
    limit: usize,
    preview_size: usize,
) -> Result<Vec<u8>> {
    build_class_search("Email", query, start, limit, preview_size)
}

/// Builds a bounded server-side Calendar Search request without a body preview.
pub fn build_calendar_search(query: &str, start: usize, limit: usize) -> Result<Vec<u8>> {
    build_class_search("Calendar", query, start, limit, 0)
}

fn build_class_search(
    class: &str,
    query: &str,
    start: usize,
    limit: usize,
    preview_size: usize,
) -> Result<Vec<u8>> {
    if query.trim().is_empty() || limit == 0 || limit > 100 {
        return Err(EasError::InvalidConfiguration(
            "search query must be non-empty and limit must be 1-100".into(),
        ));
    }
    let end = start.saturating_add(limit).saturating_sub(1);
    let mut root = element("Search", "Search");
    let mut store = element("Search", "Store");
    push_text(&mut store, "Search", "Name", "Mailbox");
    let mut query_element = element("Search", "Query");
    let mut conjunction = element("Search", "And");
    push_text(&mut conjunction, "AirSync", "Class", class);
    push_text(&mut conjunction, "Search", "FreeText", query);
    query_element.push(conjunction);
    store.push(query_element);
    let mut options = element("Search", "Options");
    push_text(&mut options, "Search", "Range", format!("{start}-{end}"));
    options.push(element("Search", "DeepTraversal"));
    let mut preference = element("AirSyncBase", "BodyPreference");
    push_text(&mut preference, "AirSyncBase", "Type", "1");
    push_text(&mut preference, "AirSyncBase", "TruncationSize", preview_size.min(500).to_string());
    options.push(preference);
    store.push(options);
    root.push(store);
    encode(&root)
}

/// Parses ordered server-side mail search results.
pub fn parse_search(data: &[u8]) -> Result<Vec<SearchMail>> {
    super::mail_search::parse_mail_search(data).map(|page| page.items)
}

/// Parses ordered server-side Calendar Search results and their total count.
pub fn parse_calendar_search(data: &[u8]) -> Result<SearchCalendarPage> {
    let (items, total) = parse_search_results(data, |properties| SearchCalendar {
        long_id: String::new(),
        collection_id: None,
        server_id: None,
        fields: parse_calendar_fields(properties),
    })?;
    Ok(SearchCalendarPage { items, total })
}

fn parse_search_results<T>(
    data: &[u8],
    parse: impl Fn(&crate::wbxml::Element) -> T,
) -> Result<(Vec<T>, usize)>
where
    T: SearchResult,
{
    let Some(root) = decode(data)? else {
        return Ok((Vec::new(), 0));
    };
    let status = integer(descendant_text(&root, "Search", "Status"), 0);
    if status != 1 {
        return Err(EasError::Protocol(format!("Search status is {status}")));
    }
    let total =
        descendant_text(&root, "Search", "Total").and_then(|value| value.parse().ok()).unwrap_or(0);
    let mut output = Vec::new();
    for result in root.descendants("Search", "Result") {
        let long_id = direct_text(result, "Search", "LongId").unwrap_or_default();
        let collection_id =
            direct_text(result, "AirSync", "CollectionId").filter(|value| !value.is_empty());
        let server_id =
            direct_text(result, "AirSync", "ServerId").filter(|value| !value.is_empty());
        if let Some(properties) = result.child("Search", "Properties")
            && !long_id.is_empty()
        {
            let mut item = parse(properties);
            item.set_source(long_id, collection_id, server_id);
            output.push(item);
        }
    }
    Ok((output, total))
}

trait SearchResult {
    fn set_source(
        &mut self,
        long_id: String,
        collection_id: Option<String>,
        server_id: Option<String>,
    );
}

impl SearchResult for SearchCalendar {
    fn set_source(
        &mut self,
        long_id: String,
        collection_id: Option<String>,
        server_id: Option<String>,
    ) {
        self.long_id = long_id;
        self.collection_id = collection_id;
        self.server_id = server_id;
    }
}

/// Builds an ItemOperations fetch by LongId or collection/server IDs.
pub fn build_item_fetch(
    long_id: Option<&str>,
    collection_id: Option<&str>,
    server_id: Option<&str>,
    truncation_size: usize,
) -> Result<Vec<u8>> {
    let mut root = element("ItemOperations", "ItemOperations");
    let mut fetch = element("ItemOperations", "Fetch");
    push_text(&mut fetch, "ItemOperations", "Store", "Mailbox");
    match (long_id, collection_id, server_id) {
        (Some(long_id), _, _) if !long_id.is_empty() => {
            push_text(&mut fetch, "Search", "LongId", long_id);
        }
        (None, Some(collection), Some(server)) if !collection.is_empty() && !server.is_empty() => {
            push_text(&mut fetch, "AirSync", "CollectionId", collection);
            push_text(&mut fetch, "AirSync", "ServerId", server);
        }
        _ => {
            return Err(EasError::InvalidConfiguration(
                "item fetch requires LongId or collection/server IDs".into(),
            ));
        }
    }
    let mut options = element("ItemOperations", "Options");
    let mut preference = element("AirSyncBase", "BodyPreference");
    push_text(&mut preference, "AirSyncBase", "Type", "1");
    push_text(
        &mut preference,
        "AirSyncBase",
        "TruncationSize",
        truncation_size.min(50_000).to_string(),
    );
    push_text(&mut preference, "AirSyncBase", "AllOrNone", "0");
    options.push(preference);
    fetch.push(options);
    root.push(fetch);
    encode(&root)
}

/// Parses a full ItemOperations mail result.
pub fn parse_item_fetch(data: &[u8]) -> Result<ItemResult> {
    parse_item_properties(data).map(|result| ItemResult {
        collection_id: result.collection_id,
        server_id: result.server_id,
        fields: parse_mail_fields(&result.properties),
    })
}

/// Parses a full ItemOperations calendar result.
pub fn parse_calendar_item_fetch(data: &[u8]) -> Result<CalendarItemResult> {
    parse_item_properties(data).map(|result| CalendarItemResult {
        collection_id: result.collection_id,
        server_id: result.server_id,
        fields: parse_calendar_fields(&result.properties),
    })
}

struct ItemProperties {
    properties: crate::wbxml::Element,
    collection_id: Option<String>,
    server_id: Option<String>,
}

fn parse_item_properties(data: &[u8]) -> Result<ItemProperties> {
    let root = decode(data)?
        .ok_or_else(|| EasError::Protocol("Exchange returned an empty ItemOperations".into()))?;
    let fetch = root
        .descendant("ItemOperations", "Fetch")
        .ok_or_else(|| EasError::Protocol("ItemOperations response has no Fetch".into()))?;
    let status = integer(direct_text(fetch, "ItemOperations", "Status"), 0);
    if status != 1 {
        return Err(EasError::Protocol(format!("ItemOperations status is {status}")));
    }
    let properties = fetch
        .child("ItemOperations", "Properties")
        .cloned()
        .ok_or_else(|| EasError::Protocol("ItemOperations response has no Properties".into()))?;
    Ok(ItemProperties {
        properties,
        collection_id: direct_text(fetch, "AirSync", "CollectionId")
            .filter(|value| !value.is_empty()),
        server_id: direct_text(fetch, "AirSync", "ServerId").filter(|value| !value.is_empty()),
    })
}

/// Builds an on-demand attachment fetch.
pub fn build_attachment_fetch(file_reference: &str) -> Result<Vec<u8>> {
    if file_reference.is_empty() {
        return Err(EasError::InvalidConfiguration("attachment reference is empty".into()));
    }
    let mut root = element("ItemOperations", "ItemOperations");
    let mut fetch = element("ItemOperations", "Fetch");
    push_text(&mut fetch, "ItemOperations", "Store", "Mailbox");
    push_text(&mut fetch, "AirSyncBase", "FileReference", file_reference);
    root.push(fetch);
    encode(&root)
}

/// Parses raw or base64 attachment data from ItemOperations.
pub fn parse_attachment_fetch(data: &[u8]) -> Result<Vec<u8>> {
    let root = decode(data)?.ok_or_else(|| {
        EasError::Protocol("Exchange returned an empty attachment response".into())
    })?;
    let fetch = root
        .descendant("ItemOperations", "Fetch")
        .ok_or_else(|| EasError::Protocol("attachment response has no Fetch".into()))?;
    let status = integer(direct_text(fetch, "ItemOperations", "Status"), 0);
    if status != 1 {
        return Err(EasError::Protocol(format!("attachment fetch status is {status}")));
    }
    let value = root
        .descendant("ItemOperations", "Data")
        .or_else(|| root.descendant("AirSyncBase", "Data"))
        .ok_or_else(|| EasError::Protocol("attachment data is missing".into()))?;
    if let Some(opaque) = value.opaque_content() {
        return Ok(opaque.to_vec());
    }
    base64::engine::general_purpose::STANDARD
        .decode(value.text_content())
        .map_err(|_| EasError::Protocol("attachment data is not valid base64".into()))
}
