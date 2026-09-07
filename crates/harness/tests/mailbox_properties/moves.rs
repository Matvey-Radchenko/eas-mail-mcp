use super::{
    build_folder_sync, build_item_fetch, default_policy, encode, item_response, mail_change,
    mailbox, mutation, read,
};
use super::{change, fetch, initialized, page, source};
use eas_mail_mcp::ErrorCode;
use eas_mail_mcp::backend::AccountBackend as _;
use eas_mail_mcp_harness::ExpectedCall;
use eas_mail_protocol::Command;
use eas_mail_protocol::protocol::MailPatch;
use eas_mail_protocol::protocol::build_move;
use eas_mail_protocol::wbxml::Element;

fn folders_with_archive() -> anyhow::Result<Vec<u8>> {
    let mut root = Element::new("FolderHierarchy", "FolderSync");
    root.push(Element::text("FolderHierarchy", "Status", "1"));
    root.push(Element::text("FolderHierarchy", "SyncKey", "folders-2"));
    let mut changes = Element::new("FolderHierarchy", "Changes");
    let mut folder = Element::new("FolderHierarchy", "Add");
    folder.push(Element::text("FolderHierarchy", "ServerId", "archive"));
    folder.push(Element::text("FolderHierarchy", "ParentId", "0"));
    folder.push(Element::text("FolderHierarchy", "DisplayName", "Archive"));
    folder.push(Element::text("FolderHierarchy", "Type", "12"));
    changes.push(folder);
    root.push(changes);
    Ok(encode(&root)?)
}

fn moved() -> anyhow::Result<Vec<u8>> {
    let mut root = Element::new("Move", "MoveItems");
    let mut response = Element::new("Move", "Response");
    response.push(Element::text("Move", "SrcMsgId", "message-2"));
    response.push(Element::text("Move", "Status", "3"));
    response.push(Element::text("Move", "DstMsgId", "moved-2"));
    root.push(response);
    Ok(encode(&root)?)
}

#[tokio::test]
async fn moving_one_item_preserves_the_write_key_for_other_synced_items() -> anyhow::Result<()> {
    let mut calls = initialized(false)?;
    if let Some(ExpectedCall::Options { headers, .. }) = calls.first_mut() {
        headers.entry("ms-asprotocolcommands".into()).or_default().push_str(",MoveItems");
    }
    let last = calls.last_mut().ok_or_else(|| anyhow::anyhow!("initial page missing"))?;
    *last = page(
        "mail-1",
        "mail-2",
        false,
        vec![
            mail_change("Add", "message-1", Some("Original")),
            mail_change("Add", "message-2", Some("Move this")),
        ],
    )?;
    calls.extend([
        fetch("message-2")?,
        read(Command::FolderSync, build_folder_sync("folders-1")?, folders_with_archive()?),
        mutation(Command::MoveItems, build_move("inbox", "message-2", "archive")?, moved()?),
        fetch("message-1")?,
        change("mail-2", "mail-3", &MailPatch::Read(true), 1)?,
        fetch("message-2")?,
        read(
            Command::ItemOperations,
            build_item_fetch(None, Some("archive"), Some("moved-2"), 1)?,
            item_response()?,
        ),
    ]);
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    mailbox.list_mail(None).await?;
    let destination = mailbox.move_mail(&source("message-2"), "archive").await?;
    mailbox.mark_read(&source("message-1"), true).await?;
    assert_eq!(
        mailbox.mark_read(&source("message-2"), true).await.map_err(|error| error.envelope.code),
        Err(ErrorCode::SyncStale)
    );
    assert_eq!(
        mailbox.mark_read(&destination, true).await.map_err(|error| error.envelope.code),
        Err(ErrorCode::FeatureUnavailable)
    );
    transport.verify_complete()?;
    Ok(())
}
