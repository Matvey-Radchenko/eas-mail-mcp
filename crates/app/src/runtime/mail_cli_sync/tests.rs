mod readiness;
mod transport;

use std::collections::BTreeMap;
use std::sync::Arc;

use eas_mail_protocol::protocol::{
    build_folder_sync, build_item_fetch, build_sync, evaluate_policy,
};
use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{CollectionKind, Command, MailFields, ProfileKey, RequestSafety};

use crate::backend::{BackendMail, EasMailbox, MailSource};
use crate::{
    AccountConfig, ErrorCode, JournalFilter, MemorySecretStore, RandomIds, Runtime, SqliteJournal,
    SystemClock,
};
use transport::{Expected, Script};

fn fixture(
    calls: Vec<Expected>,
    write_enabled: bool,
) -> anyhow::Result<(tempfile::TempDir, Runtime, Arc<Script>)> {
    let dir = tempfile::tempdir()?;
    let transport = Arc::new(Script::new(calls));
    let backend = EasMailbox::with_transport(
        "work".into(),
        AccountConfig {
            profile: ProfileKey::default(),
            email: "user@example.invalid".into(),
            username: "user".into(),
            enabled: true,
            write_enabled,
        },
        Arc::new(MemorySecretStore::default()),
        transport.clone(),
        123,
        Some(evaluate_policy(&BTreeMap::new())),
    )?;
    let runtime = Runtime::with_dependencies(
        vec![Arc::new(backend)],
        Arc::new(SqliteJournal::open(&dir.path().join("operations.sqlite"))?),
        Arc::new(SystemClock),
        Arc::new(RandomIds),
        vec![7; 32],
        dir.path().join("attachments"),
    )?;
    Ok((dir, runtime, transport))
}

fn reference(runtime: &Runtime, source: MailSource) -> crate::Result<String> {
    runtime.references.insert_mail(BackendMail {
        account_id: "work".into(),
        folder_id: "inbox".into(),
        source,
        fields: MailFields::default(),
    })
}

fn item_source() -> MailSource {
    MailSource::Item { folder_id: "inbox".into(), server_id: "message-1".into() }
}

fn fetch(subject: &str, long_id: bool) -> anyhow::Result<Expected> {
    let body = if long_id {
        build_item_fetch(Some("search-1"), None, None, 1)?
    } else {
        build_item_fetch(None, Some("inbox"), Some("message-1"), 1)?
    };
    let mut root = Element::new("ItemOperations", "ItemOperations");
    let mut fetch = Element::new("ItemOperations", "Fetch");
    fetch.push(Element::text("ItemOperations", "Status", "1"));
    let mut properties = Element::new("ItemOperations", "Properties");
    properties.push(Element::text("Email", "Subject", subject));
    fetch.push(properties);
    root.push(fetch);
    Ok(Expected {
        command: Command::ItemOperations,
        body,
        response: encode(&root)?,
        safety: RequestSafety::RetrySafe,
    })
}

fn folder_read() -> anyhow::Result<Expected> {
    let mut root = Element::new("FolderHierarchy", "FolderSync");
    root.push(Element::text("FolderHierarchy", "Status", "1"));
    root.push(Element::text("FolderHierarchy", "SyncKey", "folders-1"));
    let mut changes = Element::new("FolderHierarchy", "Changes");
    for id in ["inbox", "archive"] {
        let mut folder = Element::new("FolderHierarchy", "Add");
        folder.push(Element::text("FolderHierarchy", "ServerId", id));
        folder.push(Element::text("FolderHierarchy", "ParentId", "0"));
        folder.push(Element::text("FolderHierarchy", "DisplayName", id));
        folder.push(Element::text("FolderHierarchy", "Type", "12"));
        changes.push(folder);
    }
    root.push(changes);
    Ok(Expected {
        command: Command::FolderSync,
        safety: RequestSafety::RetrySafe,
        body: build_folder_sync("0")?,
        response: encode(&root)?,
    })
}

fn sync_read(initial: bool, include_item: bool) -> anyhow::Result<Expected> {
    let mut root = Element::new("AirSync", "Sync");
    let mut collections = Element::new("AirSync", "Collections");
    let mut collection = Element::new("AirSync", "Collection");
    collection.push(Element::text("AirSync", "Status", "1"));
    collection.push(Element::text("AirSync", "SyncKey", if initial { "mail-1" } else { "mail-2" }));
    if include_item {
        let mut commands = Element::new("AirSync", "Commands");
        let mut add = Element::new("AirSync", "Add");
        add.push(Element::text("AirSync", "ServerId", "message-1"));
        add.push(Element::new("AirSync", "ApplicationData"));
        commands.push(add);
        collection.push(commands);
    }
    collections.push(collection);
    root.push(collections);
    Ok(Expected {
        command: Command::Sync,
        safety: RequestSafety::RetrySafe,
        body: build_sync(
            "inbox",
            if initial { "0" } else { "mail-1" },
            CollectionKind::Mail,
            5,
            500,
        )?,
        response: encode(&root)?,
    })
}

#[tokio::test]
async fn duplicate_references_sync_only_the_selected_folder_once_without_a_journal_write()
-> anyhow::Result<()> {
    let calls = vec![
        fetch("Original", false)?,
        fetch("Original", false)?,
        folder_read()?,
        sync_read(true, false)?,
        sync_read(false, true)?,
        fetch("Original", false)?,
        fetch("Original", false)?,
    ];
    let (_directory, runtime, transport) = fixture(calls, true)?;
    let reference = reference(&runtime, item_source())?;
    runtime.sync_cli_mail_folders(&[reference.clone(), reference]).await?;
    assert!(runtime.journal.list(&JournalFilter::default())?.is_empty());
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn changed_metadata_after_sync_requires_a_fresh_review() -> anyhow::Result<()> {
    let calls = vec![
        fetch("Original", false)?,
        folder_read()?,
        sync_read(true, false)?,
        sync_read(false, true)?,
        fetch("Changed", false)?,
    ];
    let (_directory, runtime, transport) = fixture(calls, true)?;
    let reference = reference(&runtime, item_source())?;
    assert_eq!(
        runtime.sync_cli_mail_folders(&[reference]).await.map_err(|error| error.envelope.code),
        Err(ErrorCode::SyncStale)
    );
    assert!(runtime.journal.list(&JournalFilter::default())?.is_empty());
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn vanished_locator_does_not_select_a_replacement_or_fetch_again() -> anyhow::Result<()> {
    let calls = vec![
        fetch("Original", false)?,
        folder_read()?,
        sync_read(true, false)?,
        sync_read(false, false)?,
    ];
    let (_directory, runtime, transport) = fixture(calls, true)?;
    let reference = reference(&runtime, item_source())?;
    assert_eq!(
        runtime.sync_cli_mail_folders(&[reference]).await.map_err(|error| error.envelope.code),
        Err(ErrorCode::SyncStale)
    );
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn disabled_writes_and_unresolved_search_references_never_sync() -> anyhow::Result<()> {
    let (_directory, runtime, transport) = fixture(Vec::new(), false)?;
    let reference = reference(&runtime, item_source())?;
    assert_eq!(
        runtime.sync_cli_mail_folders(&[reference]).await.map_err(|error| error.envelope.code),
        Err(ErrorCode::ValidationFailed)
    );
    transport.verify_complete()?;
    let (_directory, runtime, transport) = fixture(vec![fetch("Original", true)?], true)?;
    let reference = self::reference(&runtime, MailSource::LongId("search-1".into()))?;
    assert_eq!(
        runtime.sync_cli_mail_folders(&[reference]).await.map_err(|error| error.envelope.code),
        Err(ErrorCode::FeatureUnavailable)
    );
    transport.verify_complete()?;
    Ok(())
}
