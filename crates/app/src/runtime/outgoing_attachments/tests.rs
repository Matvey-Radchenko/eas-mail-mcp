use std::fs;

use super::*;
use crate::model::MailSendInput;

fn input(path: &Path) -> OutgoingAttachmentInput {
    OutgoingAttachmentInput {
        path: path.to_string_lossy().into(),
        filename: None,
        content_type: None,
    }
}

#[test]
fn reads_exact_bytes_and_rejects_unsafe_inputs() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("отчёт.bin");
    fs::write(&path, [0, 255, 13, 10])?;
    let prepared = prepare(&[input(&path)])?;
    let attachment = prepared.first().ok_or_else(|| anyhow::anyhow!("missing attachment"))?;
    assert_eq!(attachment.bytes, [0, 255, 13, 10]);
    assert_eq!(attachment.filename, "отчёт.bin");
    assert_eq!(attachment.content_type, "application/octet-stream");
    assert!(prepare(&[input(directory.path())]).is_err());
    assert!(prepare(&[input(Path::new("relative"))]).is_err());
    assert!(prepare(&[input(&path.with_extension("missing"))]).is_err());
    let mut invalid = input(&path);
    invalid.filename = Some("evil\r\nHeader".into());
    assert!(prepare(&[invalid]).is_err());
    let mut invalid = input(&path);
    invalid.content_type = Some("application/pdf; injected=value".into());
    assert!(prepare(&[invalid]).is_err());
    Ok(())
}

#[test]
fn attachment_count_and_aggregate_bytes_are_bounded() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("empty.bin");
    fs::write(&path, [])?;
    assert_eq!(
        prepare(&vec![input(&path); MAX_OUTGOING_ATTACHMENTS])?.len(),
        MAX_OUTGOING_ATTACHMENTS
    );
    assert!(prepare(&vec![input(&path); MAX_OUTGOING_ATTACHMENTS + 1]).is_err());
    File::options().write(true).open(&path)?.set_len(MAX_ATTACHMENT_BYTES as u64 + 1)?;
    assert!(prepare(&[input(&path)]).is_err());
    File::options().write(true).open(&path)?.set_len(MAX_ATTACHMENT_BYTES as u64 / 2 + 1)?;
    assert!(prepare(&[input(&path), input(&path)]).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_symbolic_links_and_fifo_without_opening_them() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir()?;
    let target = directory.path().join("target");
    let link = directory.path().join("link");
    fs::write(&target, b"secret")?;
    symlink(&target, &link)?;
    assert!(prepare(&[input(&link)]).is_err());
    let fifo = directory.path().join("fifo");
    assert!(std::process::Command::new("mkfifo").arg(&fifo).status()?.success());
    assert!(prepare(&[input(&fifo)]).is_err());
    Ok(())
}

#[test]
fn canonical_input_preserves_legacy_and_detects_same_size_changes() -> anyhow::Result<()> {
    let legacy = r#"{"account_id":"a","to":["x@example.invalid"],"cc":[],"bcc":[],"subject":"s","body":"b","idempotency_key":"id"}"#;
    let input: MailSendInput = serde_json::from_str(legacy)?;
    assert_eq!(serde_json::to_string(&payload(&input, &[]))?, legacy);
    let mut attachment = MimeAttachment {
        filename: "file.bin".into(),
        content_type: "application/octet-stream".into(),
        bytes: vec![1, 2],
    };
    let first = serde_json::to_vec(&payload(&input, &[attachment.clone()]))?;
    attachment.bytes = vec![3, 4];
    let second = serde_json::to_vec(&payload(&input, &[attachment]))?;
    assert_ne!(first, second);
    assert!(String::from_utf8(first)?.contains("sha256"));
    Ok(())
}

#[test]
fn preview_detects_replaced_file_and_uses_the_prepared_buffer() -> anyhow::Result<()> {
    use super::super::mail_write_preview::{message_preview, send_message};
    use super::super::write_preview;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("file.bin");
    fs::write(&path, [1, 2])?;
    let input: MailSendInput = serde_json::from_value(serde_json::json!({
        "account_id":"a", "to":["to@example.invalid"], "subject":"s", "body":"b", "idempotency_key":"id",
        "attachments":[{"path":path}]
    }))?;
    let mut message = send_message(&input);
    message.attachments = prepare(&input.attachments)?;
    let preview = message_preview("mail_send", "a", &message);
    fs::write(&path, [3, 4])?;
    assert_eq!(message.attachments.first().map(|a| a.bytes.as_slice()), Some([1, 2].as_slice()));
    message.attachments = prepare(&input.attachments)?;
    assert!(
        write_preview::verify(
            &message_preview("mail_send", "a", &message),
            Some(&preview.fingerprint()?)
        )
        .is_err_and(|e| e.envelope.code == ErrorCode::SyncStale)
    );
    Ok(())
}

#[test]
fn empty_reply_and_forward_attachments_keep_legacy_canonical_json() -> anyhow::Result<()> {
    let reply = r#"{"mail_ref":"ref","body":"b","reply_all":false,"idempotency_key":"id"}"#;
    let reply_input: crate::model::MailReplyInput = serde_json::from_str(reply)?;
    assert_eq!(serde_json::to_string(&payload(&reply_input, &[]))?, reply);
    let forward =
        r#"{"mail_ref":"ref","to":["x@y"],"cc":[],"bcc":[],"body":"b","idempotency_key":"id"}"#;
    let forward_input: crate::model::MailForwardInput = serde_json::from_str(forward)?;
    assert_eq!(serde_json::to_string(&payload(&forward_input, &[]))?, forward);
    Ok(())
}
