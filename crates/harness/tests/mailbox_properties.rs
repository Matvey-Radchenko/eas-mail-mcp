#[expect(dead_code, reason = "shared integration-test support is compiled once per test binary")]
mod support;

use std::time::Duration;

use eas_mail_mcp::ErrorCode;
use eas_mail_mcp::backend::{AccountBackend as _, MailSource};
use eas_mail_mcp_harness::{ExpectedCall, ScriptedFailure};
use eas_mail_protocol::protocol::{
    MailPatch, build_folder_sync, build_item_fetch, build_mail_change, build_sync,
};
use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{CollectionKind, Command, Patch};

use support::{
    default_policy, folder_response, item_response, mail_change, mailbox, mutation,
    mutation_response, options, read, sync_response,
};

fn source(id: &str) -> MailSource {
    MailSource::Item { folder_id: "inbox".into(), server_id: id.into() }
}

fn fetch(id: &str) -> anyhow::Result<ExpectedCall> {
    Ok(read(
        Command::ItemOperations,
        build_item_fetch(None, Some("inbox"), Some(id), 1)?,
        item_response()?,
    ))
}

fn page(key: &str, next: &str, more: bool, items: Vec<Element>) -> anyhow::Result<ExpectedCall> {
    Ok(read(
        Command::Sync,
        build_sync("inbox", key, CollectionKind::Mail, 5, 500)?,
        sync_response(next, 1, more, items)?,
    ))
}

fn initialized(more: bool) -> anyhow::Result<Vec<ExpectedCall>> {
    Ok(vec![
        options(),
        read(Command::FolderSync, build_folder_sync("0")?, folder_response("folders-1", true)?),
        page("0", "mail-1", false, Vec::new())?,
        page("mail-1", "mail-2", more, vec![mail_change("Add", "message-1", Some("Original"))])?,
    ])
}

fn change(key: &str, next: &str, patch: &MailPatch, status: u16) -> anyhow::Result<ExpectedCall> {
    Ok(mutation(
        Command::Sync,
        build_mail_change("inbox", "message-1", key, patch)?,
        mutation_response(next, status)?,
    ))
}

#[tokio::test]
async fn fresh_property_writes_never_initialize_or_scan_a_folder() -> anyhow::Result<()> {
    let calls = vec![
        options(),
        fetch("message-1")?,
        fetch("message-1")?,
        fetch("message-1")?,
        fetch("message-1")?,
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    let source = source("message-1");
    assert_eq!(
        mailbox.mark_read(&source, true).await.map_err(|error| error.envelope.code),
        Err(ErrorCode::FeatureUnavailable)
    );
    assert_eq!(
        mailbox
            .set_mail_categories(&source, &["Project".into()])
            .await
            .map_err(|error| error.envelope.code),
        Err(ErrorCode::FeatureUnavailable)
    );
    assert_eq!(
        mailbox.set_mail_flag(&source, 0).await.map_err(|error| error.envelope.code),
        Err(ErrorCode::FeatureUnavailable)
    );
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn incomplete_initial_listing_cannot_supply_a_write_key() -> anyhow::Result<()> {
    let mut calls = initialized(true)?;
    let mut failed = page("mail-2", "mail-3", false, Vec::new())?;
    if let ExpectedCall::Command { failure, .. } = &mut failed {
        *failure = Some(ScriptedFailure::Network);
    }
    calls.extend([failed, fetch("message-1")?]);
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    assert!(mailbox.list_mail(None).await.is_err());
    assert_eq!(
        mailbox.mark_read(&source("message-1"), true).await.map_err(|error| error.envelope.code),
        Err(ErrorCode::FeatureUnavailable)
    );
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn sequential_changes_advance_the_key_and_preserve_the_cached_message() -> anyhow::Result<()>
{
    let mut calls = initialized(false)?;
    let categories = vec!["Project".into()];
    let clear_flag = MailPatch::Flag { status: 0, previous: None, updated_at: chrono::Utc::now() };
    calls.extend([
        fetch("message-1")?,
        change("mail-2", "mail-3", &MailPatch::Read(true), 1)?,
        fetch("message-1")?,
        change("mail-3", "mail-4", &MailPatch::Categories(categories.clone()), 1)?,
        fetch("message-1")?,
        fetch("message-1")?,
        change("mail-4", "mail-5", &clear_flag, 1)?,
        page("mail-5", "mail-6", false, Vec::new())?,
    ]);
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    mailbox.list_mail(None).await?;
    let source = source("message-1");
    mailbox.mark_read(&source, true).await?;
    mailbox.set_mail_categories(&source, &categories).await?;
    mailbox.set_mail_flag(&source, 0).await?;
    let cached = mailbox.list_mail(None).await?;
    let mail = cached.first().ok_or_else(|| anyhow::anyhow!("cached item missing"))?;
    assert_eq!(mail.fields.subject, Patch::Value("Original".into()));
    assert_eq!(mail.fields.is_read, Patch::Value(true));
    assert_eq!(mail.fields.categories, Patch::Value(categories));
    assert_eq!(mail.fields.flag, Patch::Value(Element::new("Email", "Flag")));
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn stale_item_or_collection_is_not_retried_implicitly() -> anyhow::Result<()> {
    for status in [3, 8] {
        let mut calls = initialized(false)?;
        let response = if status == 3 {
            let mut root = Element::new("AirSync", "Sync");
            root.push(Element::text("AirSync", "Status", "3"));
            mutation(
                Command::Sync,
                build_mail_change("inbox", "message-1", "mail-2", &MailPatch::Read(true))?,
                encode(&root)?,
            )
        } else {
            change("mail-2", "mail-3", &MailPatch::Read(true), status)?
        };
        calls.extend([fetch("message-1")?, response, fetch("message-1")?]);
        let (mailbox, transport) = mailbox(calls, default_policy())?;
        mailbox.list_mail(None).await?;
        let source = source("message-1");
        assert_eq!(
            mailbox.mark_read(&source, true).await.map_err(|error| error.envelope.code),
            Err(ErrorCode::SyncStale)
        );
        assert_eq!(
            mailbox.mark_read(&source, false).await.map_err(|error| error.envelope.code),
            Err(if status == 3 { ErrorCode::FeatureUnavailable } else { ErrorCode::SyncStale })
        );
        transport.verify_complete()?;
    }
    Ok(())
}

#[tokio::test]
async fn unknown_or_cancelled_change_cannot_leave_a_reusable_key() -> anyhow::Result<()> {
    for cancelled in [false, true] {
        let mut calls = initialized(false)?;
        let mut interrupted = change("mail-2", "mail-3", &MailPatch::Read(true), 1)?;
        if let ExpectedCall::Command { failure, delay, .. } = &mut interrupted {
            if cancelled {
                *delay = Duration::from_millis(100);
            } else {
                *failure = Some(ScriptedFailure::OutcomeUnknown);
            }
        }
        calls.extend([fetch("message-1")?, interrupted, fetch("message-1")?]);
        let (mailbox, transport) = mailbox(calls, default_policy())?;
        mailbox.list_mail(None).await?;
        let source = source("message-1");
        if cancelled {
            assert!(
                tokio::time::timeout(Duration::from_millis(10), mailbox.mark_read(&source, true))
                    .await
                    .is_err()
            );
        } else {
            assert_eq!(
                mailbox.mark_read(&source, true).await.map_err(|error| error.envelope.code),
                Err(ErrorCode::OutcomeUnknown)
            );
        }
        assert_eq!(
            mailbox.mark_read(&source, false).await.map_err(|error| error.envelope.code),
            Err(ErrorCode::FeatureUnavailable)
        );
        transport.verify_complete()?;
    }
    Ok(())
}

#[tokio::test]
async fn readable_item_outside_the_synced_collection_is_not_changed() -> anyhow::Result<()> {
    let mut calls = initialized(false)?;
    calls.push(fetch("other-message")?);
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    mailbox.list_mail(None).await?;
    assert_eq!(
        mailbox
            .mark_read(&source("other-message"), true)
            .await
            .map_err(|error| error.envelope.code),
        Err(ErrorCode::SyncStale)
    );
    transport.verify_complete()?;
    Ok(())
}

#[path = "mailbox_properties/moves.rs"]
mod moves;

#[path = "mailbox_properties/rejections.rs"]
mod rejections;
