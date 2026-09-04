use std::io::{self, Write};

use mail_builder::MessageBuilder;

use super::build_mime_message;
use crate::{EasError, Result};

/// Maximum number of new local attachments in one compose operation.
pub const MAX_OUTGOING_ATTACHMENTS: usize = 20;
/// Maximum combined unencoded attachment bytes (25 MiB).
pub const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;
/// Maximum complete outgoing MIME message size (35 MiB).
pub const MAX_MIME_BYTES: usize = 35 * 1024 * 1024;

/// An attachment already read into bounded operation-local memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimeAttachment {
    /// Display filename, without directory components or control characters.
    pub filename: String,
    /// MIME type without parameters.
    pub content_type: String,
    /// Exact bytes to transmit; never written to the journal.
    pub bytes: Vec<u8>,
}

impl MimeAttachment {
    /// Validates untrusted display metadata before it enters MIME headers.
    pub fn validate_metadata(filename: &str, content_type: &str) -> Result<()> {
        if filename.is_empty()
            || filename.len() > 255
            || filename.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
            || matches!(filename, "." | "..")
        {
            return Err(invalid("attachment filename is invalid"));
        }
        let Some((kind, subtype)) = content_type.split_once('/') else {
            return Err(invalid("attachment content_type must be a MIME type without parameters"));
        };
        if content_type.len() > 127 || !mime_token(kind) || !mime_token(subtype) {
            return Err(invalid("attachment content_type must be a MIME type without parameters"));
        }
        Ok(())
    }
}

/// Header and text-body fields shared by SendMail, SmartReply and SmartForward.
pub struct MimeMessage<'a> {
    /// Sender email address.
    pub sender: &'a str,
    /// To recipients.
    pub to: &'a [String],
    /// Cc recipients.
    pub cc: &'a [String],
    /// Bcc recipients.
    pub bcc: &'a [String],
    /// Subject text.
    pub subject: &'a str,
    /// Plain-text message body.
    pub body: &'a str,
}

/// Builds bounded multipart MIME, preserving the legacy wire form without attachments.
pub fn build_mime_with_attachments(
    message: MimeMessage<'_>,
    attachments: &[MimeAttachment],
) -> Result<Vec<u8>> {
    let plain = build_mime_message(
        message.sender,
        message.to,
        message.cc,
        message.bcc,
        message.subject,
        message.body,
    )?;
    if attachments.is_empty() {
        return if plain.len() <= MAX_MIME_BYTES { Ok(plain) } else { Err(mime_limit()) };
    }
    validate_attachments(attachments)?;
    let mut builder = MessageBuilder::new()
        .from(message.sender)
        .to(message.to.iter().map(String::as_str).collect::<Vec<_>>())
        .subject(message.subject)
        .text_body(message.body);
    if !message.cc.is_empty() {
        builder = builder.cc(message.cc.iter().map(String::as_str).collect::<Vec<_>>());
    }
    if !message.bcc.is_empty() {
        builder = builder.bcc(message.bcc.iter().map(String::as_str).collect::<Vec<_>>());
    }
    for attachment in attachments {
        builder = builder.attachment(
            attachment.content_type.as_str(),
            attachment.filename.as_str(),
            attachment.bytes.as_slice(),
        );
    }
    let mut output = BoundedMime(Vec::new());
    builder.write_to(&mut output).map_err(|_| mime_limit())?;
    Ok(output.0)
}

fn validate_attachments(attachments: &[MimeAttachment]) -> Result<()> {
    if attachments.len() > MAX_OUTGOING_ATTACHMENTS {
        return Err(invalid("at most 20 local attachments are supported"));
    }
    let mut remaining = MAX_ATTACHMENT_BYTES;
    for attachment in attachments {
        MimeAttachment::validate_metadata(&attachment.filename, &attachment.content_type)?;
        remaining = remaining
            .checked_sub(attachment.bytes.len())
            .ok_or_else(|| invalid("attachments exceed the 25 MiB combined byte limit"))?;
    }
    Ok(())
}

fn mime_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|c| c.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&c))
}

fn invalid(message: &str) -> EasError {
    EasError::InvalidConfiguration(message.into())
}

fn mime_limit() -> EasError {
    invalid("outgoing MIME exceeds the 35 MiB byte limit")
}

struct BoundedMime(Vec<u8>);

impl Write for BoundedMime {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_MIME_BYTES.saturating_sub(self.0.len()) {
            return Err(io::Error::other("MIME size limit exceeded"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
