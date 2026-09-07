use crate::wbxml::{Element, decode};
use crate::{MutationResult, Result};

use super::mutation_response::{child, malformed, required_text, status, text};

// MS-ASCMD 2.2.3.177.17. Status 14 may follow a successfully processed mutation;
// it is not proof of rejection and therefore must remain an unknown outcome.
const STATUSES: &[u16] = &[1, 3, 4, 5, 6, 7, 8, 9, 12, 13, 15, 16];

struct Expected<'a> {
    collection: &'a str,
    command: &'a str,
    identifier: &'a str,
}

pub(crate) fn parse_for(
    data: &[u8],
    collection: &str,
    command: &str,
    identifier: &str,
) -> Result<MutationResult> {
    parse(data, Some(Expected { collection, command, identifier }))
}

pub(super) fn parse_unbound(data: &[u8]) -> Result<MutationResult> {
    parse(data, None)
}

fn parse(data: &[u8], expected: Option<Expected<'_>>) -> Result<MutationResult> {
    let root = decode(data)?.ok_or_else(|| malformed("empty Calendar Sync acknowledgement"))?;
    if root.namespace != "AirSync" || root.name != "Sync" {
        return Err(malformed("unexpected Calendar Sync acknowledgement root"));
    }
    let root_status = if child(&root, "AirSync", "Status")?.is_some() {
        status(&root, "AirSync", STATUSES)?
    } else {
        1
    };
    let collections = child(&root, "AirSync", "Collections")?;
    let Some(collections) = collections else {
        return if root_status != 1 {
            Ok(MutationResult { status: root_status, sync_key: None, server_id: None })
        } else {
            Err(malformed("missing Calendar Sync collections"))
        };
    };
    if collections.children().count() != 1 {
        return Err(malformed("Calendar Sync must acknowledge exactly one collection"));
    }
    let collection = child(collections, "AirSync", "Collection")?
        .ok_or_else(|| malformed("missing Calendar Sync collection"))?;
    let collection_id = required_text(collection, "AirSync", "CollectionId")?;
    if expected.as_ref().is_some_and(|value| value.collection != collection_id) {
        return Err(malformed("Calendar Sync acknowledges a different collection"));
    }
    let collection_status = status(collection, "AirSync", STATUSES)?;
    let sync_key = text(collection, "AirSync", "SyncKey")?;
    if root_status == 1
        && collection_status == 1
        && sync_key.as_ref().is_none_or(|key| key.is_empty() || key == "0")
    {
        return Err(malformed("successful Calendar Sync has no usable SyncKey"));
    }
    let response = response(collection)?;
    let item = response.map(|response| parse_item(response, expected.as_ref())).transpose()?;
    let result_status = [root_status, collection_status, item.as_ref().map_or(1, |item| item.0)]
        .into_iter()
        .find(|value| *value != 1)
        .unwrap_or(1);
    if result_status == 1
        && expected.as_ref().is_some_and(|value| value.command == "Add")
        && item.is_none()
    {
        return Err(malformed("Calendar Add acknowledgement is missing"));
    }
    Ok(MutationResult { status: result_status, sync_key, server_id: item.and_then(|item| item.1) })
}

fn response(collection: &Element) -> Result<Option<&Element>> {
    let Some(responses) = child(collection, "AirSync", "Responses")? else { return Ok(None) };
    if responses.children().count() > 1 {
        return Err(malformed("duplicate Calendar Sync mutation acknowledgement"));
    }
    Ok(responses.children().next())
}

fn parse_item(
    response: &Element,
    expected: Option<&Expected<'_>>,
) -> Result<(u16, Option<String>)> {
    if response.namespace != "AirSync"
        || !matches!(response.name.as_str(), "Add" | "Change" | "Delete")
        || expected.is_some_and(|value| value.command != response.name)
    {
        return Err(malformed("Calendar Sync acknowledges a different mutation"));
    }
    let value = status(response, "AirSync", STATUSES)?;
    let identifier_name = if response.name == "Add" { "ClientId" } else { "ServerId" };
    let identifier = if response.name == "Add" {
        Some(required_text(response, "AirSync", identifier_name)?)
    } else {
        text(response, "AirSync", identifier_name)?
    };
    if identifier.as_ref().is_some_and(String::is_empty)
        || expected.is_some_and(|expected| {
            identifier.as_deref().is_some_and(|identifier| expected.identifier != identifier)
        })
    {
        return Err(malformed("Calendar Sync acknowledges a different item"));
    }
    let server_id = text(response, "AirSync", "ServerId")?;
    if response.name == "Add" && value == 1 && server_id.as_ref().is_none_or(String::is_empty) {
        return Err(malformed("successful Calendar Add has no ServerId"));
    }
    Ok((value, server_id.filter(|value| !value.is_empty())))
}
