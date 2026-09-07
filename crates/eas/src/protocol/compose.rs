use crate::wbxml::encode;
use crate::{EasError, MutationResult, Result};

use super::tree::{element, opaque_element, push_text};

/// Existing message reference used by SmartReply or SmartForward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeSource<'a> {
    /// Search LongId reference.
    LongId(&'a str),
    /// Synchronized collection and item identifiers.
    Item {
        /// Source folder identifier.
        folder_id: &'a str,
        /// Source message identifier.
        item_id: &'a str,
    },
}

/// Builds a small RFC 5322 UTF-8 plain-text message without local attachments.
pub fn build_mime_message(
    sender: &str,
    to: &[String],
    cc: &[String],
    bcc: &[String],
    subject: &str,
    body: &str,
) -> Result<Vec<u8>> {
    for value in std::iter::once(sender)
        .chain(to.iter().map(String::as_str))
        .chain(cc.iter().map(String::as_str))
        .chain(bcc.iter().map(String::as_str))
        .chain(std::iter::once(subject))
    {
        if value.contains(['\r', '\n']) {
            return Err(EasError::InvalidConfiguration("mail header contains a newline".into()));
        }
    }
    if to.is_empty() && cc.is_empty() && bcc.is_empty() {
        return Err(EasError::InvalidConfiguration("at least one recipient is required".into()));
    }
    let mut message = String::new();
    push_header(&mut message, "From", sender);
    push_header(&mut message, "To", &to.join(", "));
    if !cc.is_empty() {
        push_header(&mut message, "Cc", &cc.join(", "));
    }
    if !bcc.is_empty() {
        push_header(&mut message, "Bcc", &bcc.join(", "));
    }
    push_header(&mut message, "Subject", subject);
    message.push_str("MIME-Version: 1.0\r\n");
    message.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    message.push_str("Content-Transfer-Encoding: 8bit\r\n\r\n");
    message.push_str(&body.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n"));
    Ok(message.into_bytes())
}

/// Builds a SendMail command with an OPAQUE MIME body.
pub fn build_send(client_id: &str, mime: Vec<u8>) -> Result<Vec<u8>> {
    let mut root = element("ComposeMail", "SendMail");
    push_text(&mut root, "ComposeMail", "ClientId", client_id);
    root.push(element("ComposeMail", "SaveInSentItems"));
    root.push(opaque_element("ComposeMail", "Mime", mime));
    encode(&root)
}

/// Builds SmartReply or SmartForward with an immutable source reference.
pub fn build_smart(
    forward: bool,
    client_id: &str,
    source: ComposeSource<'_>,
    mime: Vec<u8>,
) -> Result<Vec<u8>> {
    let mut root = element("ComposeMail", if forward { "SmartForward" } else { "SmartReply" });
    push_text(&mut root, "ComposeMail", "ClientId", client_id);
    let mut source_element = element("ComposeMail", "Source");
    match source {
        ComposeSource::LongId(value) if !value.is_empty() => {
            push_text(&mut source_element, "ComposeMail", "LongId", value);
        }
        ComposeSource::Item { folder_id, item_id }
            if !folder_id.is_empty() && !item_id.is_empty() =>
        {
            push_text(&mut source_element, "ComposeMail", "FolderId", folder_id);
            push_text(&mut source_element, "ComposeMail", "ItemId", item_id);
        }
        ComposeSource::LongId(_) | ComposeSource::Item { .. } => {
            return Err(EasError::InvalidConfiguration("compose source is empty".into()));
        }
    }
    root.push(source_element);
    root.push(element("ComposeMail", "SaveInSentItems"));
    root.push(opaque_element("ComposeMail", "Mime", mime));
    encode(&root)
}

/// Parses a compose response; only an empty successful response confirms delivery.
pub fn parse_compose(data: &[u8]) -> Result<MutationResult> {
    super::compose_response::parse(data, None)
}

fn push_header(output: &mut String, name: &str, value: &str) {
    output.push_str(name);
    output.push_str(": ");
    output.push_str(value);
    output.push_str("\r\n");
}
