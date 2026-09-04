use eas_mail_mcp::backend::{AccountBackend, MailSource};
use eas_mail_mcp::{
    ErrorCode, MailAction, MailBatchInput, MailBatchItem, MailDeleteInput, MailFlagState,
    MailGetInput, MailGetManyInput, MailListInput, MailMoveInput, MailSetCategoriesInput,
    MailSetFlagInput, MailSummary, OperationJournal, RandomIds, Runtime, SqliteJournal,
    SystemClock,
};
use eas_mail_mcp_harness::FakeBackend;
use std::sync::Arc;

fn runtime(
    backends: Vec<Arc<dyn AccountBackend>>,
    dir: &std::path::Path,
) -> anyhow::Result<Runtime> {
    Ok(Runtime::with_dependencies(
        backends,
        Arc::new(SqliteJournal::open(&dir.join("journal.sqlite"))?),
        Arc::new(SystemClock),
        Arc::new(RandomIds),
        vec![7; 32],
        dir.join("attachments"),
    )?)
}
async fn references(runtime: &Runtime) -> anyhow::Result<Vec<MailSummary>> {
    runtime
        .mail_list(MailListInput::default())
        .await
        .data
        .map(|page| page.items)
        .ok_or_else(|| anyhow::anyhow!("no fixture messages"))
}
fn uuid(n: u128) -> String {
    uuid::Uuid::from_u128(n).to_string()
}

#[tokio::test]
async fn moved_locator_replays_across_restart_and_trash_is_only_a_move() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let backend = Arc::new(FakeBackend::new("work"));
    let first = runtime(vec![backend.clone()], temp.path())?;
    let original = references(&first).await?.remove(0).mail_ref;
    let input = MailMoveInput {
        mail_ref: original.clone(),
        destination_folder_id: "archive".into(),
        idempotency_key: uuid(11),
    };
    let result = first.mail_move(input.clone()).await;
    let new_ref = result
        .data
        .and_then(|data| data.mail_ref)
        .ok_or_else(|| anyhow::anyhow!("move failed: {:?}", result.error))?;
    assert_ne!(new_ref, original);
    assert_eq!(
        first
            .mail_get(MailGetInput { mail_ref: new_ref.clone(), body_limit: None })
            .await
            .data
            .map(|mail| mail.summary.folder_id),
        Some("archive".into())
    );
    assert_eq!(
        first
            .mail_get(MailGetInput { mail_ref: original, body_limit: None })
            .await
            .error
            .map(|error| error.code),
        Some(ErrorCode::NotFound)
    );
    drop(first);
    let second = runtime(vec![backend.clone()], temp.path())?;
    assert_eq!(
        second.mail_move(input).await.data.and_then(|data| data.mail_ref),
        Some(new_ref.clone())
    );
    assert_eq!(backend.operations()?, vec!["mail_move"]);
    let deleted =
        second.mail_delete(MailDeleteInput { mail_ref: new_ref, idempotency_key: uuid(12) }).await;
    let trash_ref = deleted
        .data
        .and_then(|data| data.mail_ref)
        .ok_or_else(|| anyhow::anyhow!("trash failed: {:?}", deleted.error))?;
    assert_eq!(
        second
            .mail_get(MailGetInput { mail_ref: trash_ref, body_limit: None })
            .await
            .data
            .map(|mail| mail.summary.folder_id),
        Some("trash".into())
    );
    assert_eq!(backend.operations()?, vec!["mail_move", "mail_move"]);
    let record = SqliteJournal::open(&temp.path().join("journal.sqlite"))?
        .inspect(&uuid(12))?
        .ok_or_else(|| anyhow::anyhow!("missing operation"))?;
    assert_eq!(record.result_locator.map(|locator| locator.folder_id), Some("trash".into()));
    Ok(())
}

#[tokio::test]
async fn flag_and_categories_preserve_unrelated_mail_and_clear_explicitly() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let backend = Arc::new(FakeBackend::new("work"));
    let runtime = runtime(vec![backend.clone()], temp.path())?;
    let reference = references(&runtime).await?.remove(0).mail_ref;
    let flag = runtime
        .mail_set_flag(MailSetFlagInput {
            mail_ref: reference.clone(),
            flag: MailFlagState::Complete,
            idempotency_key: uuid(21),
        })
        .await;
    assert!(flag.error.is_none(), "{:?}", flag.error);
    let categories = runtime
        .mail_set_categories(MailSetCategoriesInput {
            mail_ref: reference.clone(),
            categories: vec!["Project".into()],
            idempotency_key: uuid(22),
        })
        .await;
    assert!(categories.error.is_none());
    let mail = runtime
        .mail_get(MailGetInput { mail_ref: reference.clone(), body_limit: None })
        .await
        .data
        .ok_or_else(|| anyhow::anyhow!("no message"))?;
    assert_eq!(mail.summary.flag, Some(MailFlagState::Complete));
    assert_eq!(mail.summary.categories, Some(vec!["Project".into()]));
    assert_eq!(mail.summary.subject, "Quarterly update");
    runtime
        .mail_set_categories(MailSetCategoriesInput {
            mail_ref: reference.clone(),
            categories: Vec::new(),
            idempotency_key: uuid(23),
        })
        .await;
    assert_eq!(
        runtime
            .mail_get(MailGetInput { mail_ref: reference, body_limit: None })
            .await
            .data
            .and_then(|mail| mail.summary.categories),
        Some(Vec::new())
    );
    Ok(())
}

#[tokio::test]
async fn batches_continue_safe_failures_but_stop_same_account_after_unknown() -> anyhow::Result<()>
{
    let temp = tempfile::tempdir()?;
    let first = Arc::new(FakeBackend::new("a").with_mail_count(3));
    let second = Arc::new(FakeBackend::new("b"));
    let runtime = runtime(vec![first.clone(), second.clone()], temp.path())?;
    let mails = references(&runtime).await?;
    let a = mails.iter().filter(|mail| mail.account_id == "a").collect::<Vec<_>>();
    let b = mails
        .iter()
        .find(|mail| mail.account_id == "b")
        .ok_or_else(|| anyhow::anyhow!("no second account"))?;
    let input = MailBatchInput {
        items: vec![
            MailBatchItem {
                mail_ref: at(&a, 0)?.mail_ref.clone(),
                idempotency_key: uuid(31),
                action: MailAction::SetFlag { flag: MailFlagState::Active },
            },
            MailBatchItem {
                mail_ref: at(&a, 1)?.mail_ref.clone(),
                idempotency_key: uuid(32),
                action: MailAction::SetCategories { categories: vec!["x".into()] },
            },
            MailBatchItem {
                mail_ref: at(&a, 2)?.mail_ref.clone(),
                idempotency_key: uuid(33),
                action: MailAction::MarkRead { is_read: true },
            },
            MailBatchItem {
                mail_ref: b.mail_ref.clone(),
                idempotency_key: uuid(34),
                action: MailAction::MarkRead { is_read: true },
            },
        ],
    };
    first.set_operation_failure(Some("mail_set_flag"), ErrorCode::ProtocolError)?;
    let result = runtime
        .mail_batch(input.clone())
        .await
        .data
        .ok_or_else(|| anyhow::anyhow!("batch failed"))?;
    assert!(at(&result.items, 0)?.error.is_some());
    assert!(
        result
            .items
            .get(1..)
            .ok_or_else(|| anyhow::anyhow!("missing batch results"))?
            .iter()
            .all(|item| item.error.is_none())
    );
    first.set_operation_failure(Some("mail_set_flag"), ErrorCode::OutcomeUnknown)?;
    let mut next = input;
    for (index, entry) in next.items.iter_mut().enumerate() {
        entry.idempotency_key = uuid(40 + index as u128);
    }
    let result =
        runtime.mail_batch(next).await.data.ok_or_else(|| anyhow::anyhow!("batch failed"))?;
    assert_eq!(
        at(&result.items, 0)?.error.as_ref().map(|error| error.code),
        Some(ErrorCode::OutcomeUnknown)
    );
    assert!(at(&result.items, 1)?.skipped && at(&result.items, 2)?.skipped);
    assert!(!at(&result.items, 3)?.skipped && at(&result.items, 3)?.error.is_none());
    let journal = SqliteJournal::open(&temp.path().join("journal.sqlite"))?;
    assert!(journal.lookup(&uuid(41))?.is_none());
    assert!(journal.lookup(&uuid(42))?.is_none());
    Ok(())
}

#[tokio::test]
async fn duplicate_batch_targets_are_rejected_before_writes_and_bulk_reads_share_budget()
-> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let backend = Arc::new(FakeBackend::new("work").with_mail_count(3));
    let runtime = runtime(vec![backend.clone()], temp.path())?;
    let refs =
        references(&runtime).await?.into_iter().map(|mail| mail.mail_ref).collect::<Vec<_>>();
    let duplicate_ref = at(&refs, 0)?.clone();
    let items = (0..2)
        .map(|index| MailBatchItem {
            mail_ref: duplicate_ref.clone(),
            idempotency_key: uuid(50 + index),
            action: MailAction::Delete,
        })
        .collect();
    assert_eq!(
        runtime.mail_batch(MailBatchInput { items }).await.error.map(|error| error.code),
        Some(ErrorCode::ValidationFailed)
    );
    assert!(backend.operations()?.is_empty());
    for index in 0..3 {
        backend.set_mail_body(
            &MailSource::Item { folder_id: "inbox".into(), server_id: format!("message-{index}") },
            "x".repeat(8000),
        )?;
    }
    let report = runtime
        .mail_get_many(MailGetManyInput {
            mail_refs: refs,
            body_limit: Some(10_000),
            total_body_limit: Some(15_000),
        })
        .await
        .data
        .ok_or_else(|| anyhow::anyhow!("bulk read failed"))?;
    let count: usize = report
        .items
        .iter()
        .filter_map(|item| item.mail.as_ref())
        .map(|mail| mail.body.chars().count())
        .sum();
    assert_eq!(count, 15_000);
    assert!(report.bodies_truncated);
    Ok(())
}

fn at<T>(items: &[T], index: usize) -> anyhow::Result<&T> {
    items.get(index).ok_or_else(|| anyhow::anyhow!("missing fixture entry"))
}

#[path = "mail_mutations/safety.rs"]
mod safety;
