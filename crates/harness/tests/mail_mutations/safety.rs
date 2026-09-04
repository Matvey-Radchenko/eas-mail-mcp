use super::*;
use eas_mail_mcp::{AttachmentDownloadInput, MailAttachmentsInput};

#[tokio::test]
async fn remote_wipe_in_batch_purges_account_and_blocks_later_writes() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let backend = Arc::new(FakeBackend::new("work").with_mail_count(2));
    let other = Arc::new(FakeBackend::new("other"));
    let runtime = runtime(vec![backend.clone(), other.clone()], temp.path())?;
    let mails = references(&runtime).await?;
    let work = mails.iter().filter(|m| m.account_id == "work").collect::<Vec<_>>();
    let other_mail = mails
        .iter()
        .find(|m| m.account_id == "other")
        .ok_or_else(|| anyhow::anyhow!("missing account"))?;
    let first = at(&work, 0)?.mail_ref.clone();
    let downloaded = download(&runtime, &first).await?;
    backend.set_operation_failure(Some("mail_set_flag"), ErrorCode::RemoteWipe)?;
    let input = MailBatchInput {
        items: vec![
            MailBatchItem {
                mail_ref: first,
                idempotency_key: uuid(801),
                action: MailAction::SetFlag { flag: MailFlagState::Complete },
            },
            MailBatchItem {
                mail_ref: at(&work, 1)?.mail_ref.clone(),
                idempotency_key: uuid(802),
                action: MailAction::MarkRead { is_read: true },
            },
            MailBatchItem {
                mail_ref: other_mail.mail_ref.clone(),
                idempotency_key: uuid(803),
                action: MailAction::MarkRead { is_read: true },
            },
        ],
    };
    let response = runtime.mail_batch(input).await;
    let data =
        response.data.ok_or_else(|| anyhow::anyhow!("batch lost report: {:?}", response.error))?;
    for item in data.items.iter().take(2) {
        assert_eq!(item.error.as_ref().map(|e| e.code), Some(ErrorCode::RemoteWipe));
    }
    assert!(at(&data.items, 2)?.error.is_none());
    assert!(backend.operations()?.is_empty());
    assert_eq!(other.operations()?, ["mail_mark_read"]);
    assert!(!std::path::Path::new(&downloaded).exists());
    let journal = SqliteJournal::open(&temp.path().join("journal.sqlite"))?;
    assert!(journal.lookup(&uuid(801))?.is_none());
    assert!(journal.lookup(&uuid(802))?.is_none());
    backend.set_operation_failure(None, ErrorCode::RemoteWipe)?;
    let blocked = runtime
        .mail_delete(MailDeleteInput {
            mail_ref: at(&work, 1)?.mail_ref.clone(),
            idempotency_key: uuid(804),
        })
        .await;
    assert_eq!(blocked.error.map(|e| e.code), Some(ErrorCode::RemoteWipe));
    assert!(backend.operations()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn disabled_write_permission_rejects_all_new_mutations_before_journaling()
-> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let backend = Arc::new(FakeBackend::new("work").with_writes_enabled(false));
    let runtime = runtime(vec![backend.clone()], temp.path())?;
    let reference = references(&runtime).await?.remove(0).mail_ref;
    let actions = [
        MailAction::Move { destination_folder_id: "archive".into() },
        MailAction::Delete,
        MailAction::SetFlag { flag: MailFlagState::Active },
        MailAction::SetCategories { categories: vec!["project".into()] },
        MailAction::MarkRead { is_read: true },
    ];
    for (index, action) in actions.into_iter().enumerate() {
        let operation_id = uuid(900 + index as u128);
        let response = runtime
            .mail_batch(MailBatchInput {
                items: vec![MailBatchItem {
                    mail_ref: reference.clone(),
                    idempotency_key: operation_id.clone(),
                    action,
                }],
            })
            .await;
        let data = response.data.ok_or_else(|| anyhow::anyhow!("missing batch report"))?;
        assert_eq!(
            at(&data.items, 0)?.error.as_ref().map(|e| e.code),
            Some(ErrorCode::ValidationFailed)
        );
        assert!(
            SqliteJournal::open(&temp.path().join("journal.sqlite"))?
                .lookup(&operation_id)?
                .is_none()
        );
    }
    assert!(backend.operations()?.is_empty());
    assert_eq!(backend.source_resolutions(), 0);
    Ok(())
}

async fn download(runtime: &Runtime, reference: &str) -> anyhow::Result<String> {
    let attachment_ref = runtime
        .mail_list_attachments(MailAttachmentsInput { mail_ref: reference.into() })
        .await
        .data
        .and_then(|v| v.attachments.into_iter().next())
        .map(|v| v.attachment_ref)
        .ok_or_else(|| anyhow::anyhow!("missing attachment"))?;
    runtime
        .mail_download_attachment(AttachmentDownloadInput { attachment_ref })
        .await
        .data
        .map(|v| v.path)
        .ok_or_else(|| anyhow::anyhow!("download failed"))
}
