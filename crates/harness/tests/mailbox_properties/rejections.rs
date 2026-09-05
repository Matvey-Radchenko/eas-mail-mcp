use super::{change, fetch, initialized, page, source};
use super::{default_policy, mail_change, mailbox, mutation};
use eas_mail_mcp::ErrorCode;
use eas_mail_mcp::backend::AccountBackend as _;
use eas_mail_protocol::protocol::{MailPatch, build_mail_change};
use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{Command, Patch};

fn accepted_other_item() -> anyhow::Result<Vec<u8>> {
    let mut root = Element::new("AirSync", "Sync");
    let mut collection = Element::new("AirSync", "Collection");
    collection.push(Element::text("AirSync", "CollectionId", "inbox"));
    collection.push(Element::text("AirSync", "Status", "1"));
    collection.push(Element::text("AirSync", "SyncKey", "mail-4"));
    let mut collections = Element::new("AirSync", "Collections");
    collections.push(collection);
    root.push(collections);
    Ok(encode(&root)?)
}

#[tokio::test]
async fn definite_rejection_advances_the_key_for_the_next_item() -> anyhow::Result<()> {
    for status in [6, 7, 8, 9] {
        let mut calls = initialized(false)?;
        let last = calls.last_mut().ok_or_else(|| anyhow::anyhow!("initial page missing"))?;
        *last = page(
            "mail-1",
            "mail-2",
            false,
            vec![
                mail_change("Add", "message-1", Some("Rejected")),
                mail_change("Add", "message-2", Some("Accepted")),
            ],
        )?;
        calls.extend([
            fetch("message-1")?,
            change("mail-2", "mail-3", &MailPatch::Read(true), status)?,
            fetch("message-2")?,
            mutation(
                Command::Sync,
                build_mail_change("inbox", "message-2", "mail-3", &MailPatch::Read(true))?,
                accepted_other_item()?,
            ),
            page("mail-4", "mail-5", false, Vec::new())?,
        ]);
        let (mailbox, transport) = mailbox(calls, default_policy())?;
        mailbox.list_mail(None).await?;
        assert_eq!(
            mailbox
                .mark_read(&source("message-1"), true)
                .await
                .map_err(|error| error.envelope.code),
            Err(if status == 8 { ErrorCode::SyncStale } else { ErrorCode::ProtocolError })
        );
        mailbox.mark_read(&source("message-2"), true).await?;
        let snapshot = mailbox.list_mail(None).await?;
        let rejected = snapshot.iter().find(|mail| mail.source == source("message-1"));
        if status == 8 {
            assert!(rejected.is_none());
        } else {
            assert_eq!(rejected.map(|mail| mail.fields.is_read.clone()), Some(Patch::Value(false)));
        }
        let accepted = snapshot.iter().find(|mail| mail.source == source("message-2"));
        assert_eq!(accepted.map(|mail| mail.fields.is_read.clone()), Some(Patch::Value(true)));
        transport.verify_complete()?;
    }
    Ok(())
}
