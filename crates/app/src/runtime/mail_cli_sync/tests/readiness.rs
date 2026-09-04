use super::{Expected, fetch, fixture, folder_read, item_source, reference, sync_read};
use crate::model::{
    MailAction, MailBatchItem, MailSetCategoriesInput, MarkReadInput, OperationState,
};
use crate::runtime::write_preview::PreparedWrite;
use crate::{ErrorCode, JournalFilter, Runtime};
use eas_mail_protocol::protocol::{MailPatch, build_mail_change};
use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{Command, RequestSafety};

fn change(mark_read: bool) -> anyhow::Result<Expected> {
    let mut root = Element::new("AirSync", "Sync");
    let mut collections = Element::new("AirSync", "Collections");
    let mut collection = Element::new("AirSync", "Collection");
    collection.push(Element::text("AirSync", "CollectionId", "inbox"));
    collection.push(Element::text("AirSync", "Status", "1"));
    collection.push(Element::text("AirSync", "SyncKey", "mail-3"));
    collections.push(collection);
    root.push(collections);
    let patch = if mark_read { MailPatch::Read(true) } else { MailPatch::Categories(Vec::new()) };
    Ok(Expected {
        command: Command::Sync,
        body: build_mail_change("inbox", "message-1", "mail-2", &patch)?,
        response: encode(&root)?,
        safety: RequestSafety::Mutation,
    })
}

async fn invoke(
    runtime: &Runtime,
    reference: &str,
    key: &str,
    mark_read: bool,
) -> (Option<ErrorCode>, Option<OperationState>) {
    if mark_read {
        let response = runtime
            .mail_mark_read(MarkReadInput {
                mail_ref: reference.into(),
                is_read: true,
                idempotency_key: key.into(),
            })
            .await;
        (response.error.map(|error| error.code), response.data.map(|data| data.status))
    } else {
        let response = runtime
            .mail_set_categories(MailSetCategoriesInput {
                mail_ref: reference.into(),
                categories: Vec::new(),
                idempotency_key: key.into(),
            })
            .await;
        (response.error.map(|error| error.code), response.data.map(|data| data.status))
    }
}

#[tokio::test]
async fn missing_sync_does_not_claim_the_uuid_for_old_or_new_mcp_writes() -> anyhow::Result<()> {
    for mark_read in [true, false] {
        let calls = vec![
            fetch("Original", false)?,
            fetch("Original", false)?,
            folder_read()?,
            sync_read(true, false)?,
            sync_read(false, true)?,
            fetch("Original", false)?,
            fetch("Original", false)?,
            fetch("Original", false)?,
            change(mark_read)?,
        ];
        let (_directory, runtime, transport) = fixture(calls, true)?;
        let reference = reference(&runtime, item_source())?;
        let key = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            invoke(&runtime, &reference, &key, mark_read).await,
            (Some(ErrorCode::FeatureUnavailable), None)
        );
        assert!(runtime.journal.lookup(&key)?.is_none());
        runtime.sync_cli_mail_folders(std::slice::from_ref(&reference)).await?;
        assert_eq!(
            invoke(&runtime, &reference, &key, mark_read).await,
            (None, Some(OperationState::Succeeded))
        );
        assert_eq!(
            invoke(&runtime, &reference, &key, mark_read).await,
            (None, Some(OperationState::Succeeded))
        );
        transport.verify_complete()?;
    }
    Ok(())
}

#[tokio::test]
async fn cli_readiness_before_approval_keeps_the_same_uuid_available_after_explicit_sync()
-> anyhow::Result<()> {
    let calls = vec![
        fetch("Original", false)?,
        fetch("Original", false)?,
        folder_read()?,
        sync_read(true, false)?,
        sync_read(false, true)?,
        fetch("Original", false)?,
        fetch("Original", false)?,
        fetch("Original", false)?,
        fetch("Original", false)?,
        change(false)?,
    ];
    let (_directory, runtime, transport) = fixture(calls, true)?;
    let reference = reference(&runtime, item_source())?;
    let entry = MailBatchItem {
        mail_ref: reference.clone(),
        idempotency_key: uuid::Uuid::new_v4().to_string(),
        action: MailAction::SetCategories { categories: Vec::new() },
    };
    assert!(matches!(runtime.prepare_cli_mail_mutation(&entry).await?, PreparedWrite::Ready(_)));
    assert_eq!(
        runtime.check_cli_mail_property(&reference).await.map_err(|error| error.envelope.code),
        Err(ErrorCode::FeatureUnavailable)
    );
    assert!(runtime.journal.list(&JournalFilter::default())?.is_empty());
    runtime.sync_cli_mail_folders(std::slice::from_ref(&reference)).await?;
    let PreparedWrite::Ready(preview) = runtime.prepare_cli_mail_mutation(&entry).await? else {
        anyhow::bail!("UUID was incorrectly claimed");
    };
    runtime.check_cli_mail_property(&reference).await?;
    let response = runtime.mail_mutation(entry.clone(), Some(&preview.fingerprint()?)).await;
    assert_eq!(response.data.map(|data| data.status), Some(OperationState::Succeeded));
    assert!(matches!(runtime.prepare_cli_mail_mutation(&entry).await?, PreparedWrite::Replay(_)));
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn readiness_for_an_item_locator_does_not_contact_exchange() -> anyhow::Result<()> {
    let (_directory, runtime, transport) = fixture(Vec::new(), true)?;
    let reference = reference(&runtime, item_source())?;
    assert_eq!(
        runtime.check_cli_mail_property(&reference).await.map_err(|error| error.envelope.code),
        Err(ErrorCode::FeatureUnavailable)
    );
    assert!(runtime.journal.list(&JournalFilter::default())?.is_empty());
    transport.verify_complete()?;
    Ok(())
}
