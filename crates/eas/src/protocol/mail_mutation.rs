use super::tree::{element, push_text};
use crate::wbxml::{Element, decode, encode};

mod flag;
use crate::{EasError, MutationResult, Result};

/// A single EAS mail property change; other message properties remain untouched.
#[derive(Debug, Clone)]
pub enum MailPatch {
    /// Replace read state.
    Read(bool),
    /// Replace flag status while preserving supported flag parameters.
    Flag {
        /// EAS flag status: zero clears, one completes, two activates.
        status: u8,
        /// Complete existing flag metadata from ItemOperations.
        previous: Option<Element>,
        /// Time of this status change, used when recording completion.
        updated_at: chrono::DateTime<chrono::Utc>,
    },
    /// Replace the category set; an empty set clears it.
    Categories(Vec<String>),
}

/// Builds exactly one minimal Email property change without requesting server changes.
pub fn build_mail_change(
    folder: &str,
    server_id: &str,
    sync_key: &str,
    patch: &MailPatch,
) -> Result<Vec<u8>> {
    let mut root = element("AirSync", "Sync");
    let mut collections = element("AirSync", "Collections");
    let mut collection = element("AirSync", "Collection");
    push_text(&mut collection, "AirSync", "SyncKey", sync_key);
    push_text(&mut collection, "AirSync", "CollectionId", folder);
    push_text(&mut collection, "AirSync", "GetChanges", "0");
    let mut commands = element("AirSync", "Commands");
    let mut change = element("AirSync", "Change");
    push_text(&mut change, "AirSync", "ServerId", server_id);
    let mut application = element("AirSync", "ApplicationData");
    application.push(patch_element(patch)?);
    change.push(application);
    commands.push(change);
    collection.push(commands);
    collections.push(collection);
    root.push(collections);
    encode(&root)
}

fn patch_element(patch: &MailPatch) -> Result<Element> {
    match patch {
        MailPatch::Read(read) => Ok(Element::text("Email", "Read", if *read { "1" } else { "0" })),
        MailPatch::Flag { status, previous, updated_at } => {
            flag::build(*status, previous.as_ref(), *updated_at)
        }
        MailPatch::Categories(categories) => {
            let mut result = element("Email", "Categories");
            for category in categories {
                push_text(&mut result, "Email", "Category", category);
            }
            Ok(result)
        }
    }
}

/// Parses a complete Sync response, including implicit success for an omitted Change response.
pub fn parse_mail_change(data: &[u8], folder: &str, server_id: &str) -> Result<MutationResult> {
    let root = decode(data)?.ok_or_else(|| malformed("empty Sync response"))?;
    if root.namespace != "AirSync" || root.name != "Sync" {
        return Err(malformed("unexpected Sync root"));
    }
    if let Some(status) = text(&root, "AirSync", "Status")? {
        require_status(&status)?;
    }
    let collections = child(&root, "AirSync", "Collections")?
        .ok_or_else(|| malformed("missing Sync collections"))?;
    let collection = child(collections, "AirSync", "Collection")?
        .ok_or_else(|| malformed("missing Sync collection"))?;
    if text(collection, "AirSync", "CollectionId")?.as_deref() != Some(folder) {
        return Err(malformed("Sync response identifies a different collection"));
    }
    require_status(
        &text(collection, "AirSync", "Status")?
            .ok_or_else(|| malformed("missing collection status"))?,
    )?;
    let sync_key = text(collection, "AirSync", "SyncKey")?
        .filter(|key| !key.is_empty() && key != "0")
        .ok_or_else(|| malformed("missing collection SyncKey"))?;
    let mut status = 1;
    if let Some(responses) = child(collection, "AirSync", "Responses")? {
        if responses.children().count() > 1 {
            return Err(malformed("duplicate Sync mutation response"));
        }
        for response in responses.children() {
            if response.namespace != "AirSync"
                || response.name != "Change"
                || text(response, "AirSync", "ServerId")?
                    .as_deref()
                    .is_some_and(|value| value != server_id)
            {
                return Err(malformed("unexpected Sync mutation response"));
            }
            status = text(response, "AirSync", "Status")?
                .and_then(|text| text.parse().ok())
                .ok_or_else(|| malformed("missing change status"))?;
            // MS-ASCMD 2.2.3.177.17. Status 14 can follow a processed request, so it
            // cannot prove rejection. Common/unknown statuses require separate semantics.
            if !matches!(status, 1 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 12 | 13 | 15 | 16) {
                return Err(malformed("unsupported or ambiguous Sync change status"));
            }
        }
    }
    Ok(MutationResult { status, sync_key: Some(sync_key), server_id: None })
}

fn require_status(status: &str) -> Result<()> {
    match status {
        "1" => Ok(()),
        "3" => Err(EasError::InvalidSyncKey),
        _ => Err(malformed("Exchange did not confirm the Sync collection")),
    }
}

/// Builds a MoveItems operation within one mailbox.
pub fn build_move(source_folder: &str, server_id: &str, destination: &str) -> Result<Vec<u8>> {
    let mut root = element("Move", "MoveItems");
    let mut item = element("Move", "Move");
    push_text(&mut item, "Move", "SrcMsgId", server_id);
    push_text(&mut item, "Move", "SrcFldId", source_folder);
    push_text(&mut item, "Move", "DstFldId", destination);
    root.push(item);
    encode(&root)
}

/// Requires the matching source and a new destination identifier after successful MoveItems.
pub fn parse_move(data: &[u8], server_id: &str) -> Result<MutationResult> {
    let root = decode(data)?.ok_or_else(|| malformed("empty MoveItems response"))?;
    if root.namespace != "Move" || root.name != "MoveItems" {
        return Err(malformed("unexpected MoveItems root"));
    }
    if text(&root, "Move", "Status")?.is_some() {
        return Err(malformed("MoveItems returned an overall failure status"));
    }
    let response =
        child(&root, "Move", "Response")?.ok_or_else(|| malformed("missing MoveItems response"))?;
    if text(response, "Move", "SrcMsgId")?.as_deref() != Some(server_id) {
        return Err(malformed("MoveItems response identifies a different message"));
    }
    let status = text(response, "Move", "Status")?
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| malformed("missing MoveItems status"))?;
    // MS-ASCMD 2.2.3.177.10 defines this exact set; even 6 is not a MoveItems status.
    if !matches!(status, 1 | 2 | 3 | 4 | 5 | 7) {
        return Err(malformed("unsupported MoveItems status"));
    }
    let destination = text(response, "Move", "DstMsgId")?.filter(|id| !id.is_empty());
    if status == 3 && destination.is_none() {
        return Err(malformed("successful MoveItems response has no destination identifier"));
    }
    Ok(MutationResult { status, sync_key: None, server_id: destination })
}

fn child<'a>(parent: &'a Element, namespace: &str, name: &str) -> Result<Option<&'a Element>> {
    let mut matches =
        parent.children().filter(|node| node.namespace == namespace && node.name == name);
    let first = matches.next();
    if matches.next().is_some() {
        return Err(malformed("duplicate mutation response field"));
    }
    Ok(first)
}

fn text(parent: &Element, namespace: &str, name: &str) -> Result<Option<String>> {
    child(parent, namespace, name)?
        .map(|value| {
            if value.children().next().is_some() {
                Err(malformed("nested mutation response scalar"))
            } else {
                Ok(value.text_content())
            }
        })
        .transpose()
}

fn malformed(message: &str) -> EasError {
    EasError::Protocol(message.into())
}

#[cfg(test)]
mod tests;
