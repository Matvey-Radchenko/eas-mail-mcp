use std::fs::{self, File, OpenOptions};
use std::io::Read as _;
use std::path::Path;

use eas_mail_protocol::protocol::{MAX_ATTACHMENT_BYTES, MAX_OUTGOING_ATTACHMENTS, MimeAttachment};
use serde::{Serialize, Serializer};
use sha2::{Digest as _, Sha256};

use crate::model::OutgoingAttachmentInput;
use crate::{AppError, ErrorCode, Result, platform};

/// Reads each file once and keeps the exact bounded bytes used by the subsequent write.
pub(super) fn prepare(inputs: &[OutgoingAttachmentInput]) -> Result<Vec<MimeAttachment>> {
    if inputs.len() > MAX_OUTGOING_ATTACHMENTS {
        return Err(invalid("at most 20 local attachments are supported"));
    }
    let mut remaining = MAX_ATTACHMENT_BYTES;
    let mut attachments = Vec::with_capacity(inputs.len());
    for input in inputs {
        let path = Path::new(&input.path);
        if !path.is_absolute() {
            return Err(invalid("attachment path must be absolute"));
        }
        let filename = input
            .filename
            .as_deref()
            .or_else(|| path.file_name()?.to_str())
            .ok_or_else(|| invalid("attachment needs a UTF-8 display filename"))?;
        let content_type = input.content_type.as_deref().unwrap_or("application/octet-stream");
        MimeAttachment::validate_metadata(filename, content_type)
            .map_err(|_| invalid("attachment filename or content_type is invalid"))?;
        let file = open_regular(path)?;
        let metadata = file.metadata().map_err(|_| invalid("cannot inspect attachment file"))?;
        if metadata.len() > remaining as u64 {
            return Err(raw_limit());
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(remaining as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid("cannot read attachment file"))?;
        remaining = remaining.checked_sub(bytes.len()).ok_or_else(raw_limit)?;
        attachments.push(MimeAttachment {
            filename: filename.into(),
            content_type: content_type.into(),
            bytes,
        });
    }
    Ok(attachments)
}

fn open_regular(path: &Path) -> Result<File> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| invalid("cannot inspect attachment file"))?;
    if platform::is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(invalid("attachment must be a regular file, not a link or reparse point"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    set_read_flags(&mut options);
    let file = options.open(path).map_err(|_| invalid("cannot open attachment file"))?;
    let opened = file.metadata().map_err(|_| invalid("cannot inspect opened attachment"))?;
    if platform::is_link_or_reparse(&opened) || !opened.is_file() {
        return Err(invalid("attachment must remain a regular file"));
    }
    Ok(file)
}

#[cfg(unix)]
fn set_read_flags(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt as _;
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
}

#[cfg(windows)]
fn set_read_flags(options: &mut OpenOptions) {
    use std::os::windows::fs::OpenOptionsExt as _;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
}

#[cfg(not(any(unix, windows)))]
fn set_read_flags(_: &mut OpenOptions) {}

#[derive(Serialize)]
pub(super) struct AttachmentDigest {
    filename: String,
    content_type: String,
    size: usize,
    sha256: String,
}

pub(super) fn digest(attachment: &MimeAttachment) -> AttachmentDigest {
    AttachmentDigest {
        filename: attachment.filename.clone(),
        content_type: attachment.content_type.clone(),
        size: attachment.bytes.len(),
        sha256: Sha256::digest(&attachment.bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    }
}

impl AttachmentDigest {
    pub(super) fn preview(&self) -> String {
        format!(
            "{}; {}; {} bytes; SHA-256 {}",
            self.filename, self.content_type, self.size, self.sha256
        )
    }
}

pub(super) struct MailPayload<'a, T> {
    input: &'a T,
    attachments: Vec<AttachmentDigest>,
}

pub(super) fn payload<'a, T>(input: &'a T, attachments: &[MimeAttachment]) -> MailPayload<'a, T> {
    MailPayload { input, attachments: attachments.iter().map(digest).collect() }
}

impl<T: Serialize> Serialize for MailPayload<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        // Preserve the exact pre-1.0 canonical JSON for existing operations without attachments.
        if self.attachments.is_empty() {
            return self.input.serialize(serializer);
        }
        #[derive(Serialize)]
        struct WithAttachments<'a, T> {
            input: &'a T,
            attachments: &'a [AttachmentDigest],
        }
        WithAttachments { input: self.input, attachments: &self.attachments }.serialize(serializer)
    }
}

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}

fn raw_limit() -> AppError {
    invalid("attachments exceed the 25 MiB combined byte limit")
}

#[cfg(test)]
mod tests;
