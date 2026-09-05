use eas_mail_mcp::{
    ApiResponse, MailAction, MailBatchInput, MailBatchItem, MailDeleteInput, MailDetail,
    MailFlagState, MailMoveInput, MailMutationResult, MailSetCategoriesInput, MailSetFlagInput,
    OperationState, Runtime,
};

use super::{SyntheticMailFixture, get, operation_id, required};

pub(super) async fn properties(runtime: &Runtime, mail: &MailDetail) -> anyhow::Result<()> {
    let mail_ref = &mail.summary.mail_ref;
    set_flag(runtime, mail_ref, MailFlagState::Active).await?;
    let active = get(runtime, mail_ref).await;
    set_flag(runtime, mail_ref, MailFlagState::Complete).await?;
    let completed = get(runtime, mail_ref).await;
    set_flag(runtime, mail_ref, MailFlagState::None).await?;
    let cleared = get(runtime, mail_ref).await?;
    anyhow::ensure!(
        active?.summary.flag == Some(MailFlagState::Active)
            && completed?.summary.flag == Some(MailFlagState::Complete)
            && cleared.summary.flag == Some(MailFlagState::None),
        "flag round trip did not return active, complete and cleared states"
    );
    let category = "EAS Mail MCP acceptance".to_owned();
    set_categories(runtime, mail_ref, vec![category.clone()]).await?;
    let categorized = get(runtime, mail_ref).await;
    set_categories(runtime, mail_ref, Vec::new()).await?;
    let cleared = get(runtime, mail_ref).await?;
    anyhow::ensure!(
        categorized?.summary.categories == Some(vec![category])
            && cleared.summary.categories == Some(Vec::new()),
        "category round trip did not return both explicit states"
    );
    Ok(())
}

async fn set_flag(runtime: &Runtime, mail_ref: &str, flag: MailFlagState) -> anyhow::Result<()> {
    confirmed(
        runtime
            .mail_set_flag(MailSetFlagInput {
                mail_ref: mail_ref.into(),
                flag,
                idempotency_key: operation_id(),
            })
            .await,
        "mail_set_flag fixture",
    )?;
    Ok(())
}

async fn set_categories(
    runtime: &Runtime,
    mail_ref: &str,
    categories: Vec<String>,
) -> anyhow::Result<()> {
    confirmed(
        runtime
            .mail_set_categories(MailSetCategoriesInput {
                mail_ref: mail_ref.into(),
                categories,
                idempotency_key: operation_id(),
            })
            .await,
        "mail_set_categories fixture",
    )?;
    Ok(())
}

pub(super) async fn trash_and_restore(
    runtime: &Runtime,
    fixture: &SyntheticMailFixture,
    original: &MailDetail,
) -> anyhow::Result<MailDetail> {
    let trashed = confirmed(
        runtime
            .mail_delete(MailDeleteInput {
                mail_ref: original.summary.mail_ref.clone(),
                idempotency_key: operation_id(),
            })
            .await,
        "mail_delete fixture to trash",
    )?;
    let trash_ref = reference(trashed)?;
    let in_trash = get(runtime, &trash_ref).await;
    let restored = confirmed(
        runtime
            .mail_move(MailMoveInput {
                mail_ref: trash_ref,
                destination_folder_id: fixture.inbox_id.clone(),
                idempotency_key: operation_id(),
            })
            .await,
        "mail_move fixture to Inbox",
    )?;
    let restored = get(runtime, &reference(restored)?).await?;
    let in_trash = in_trash?;
    anyhow::ensure!(
        in_trash.summary.folder_id == fixture.trash_id
            && in_trash.summary.subject == original.summary.subject
            && restored.summary.folder_id == fixture.inbox_id
            && restored.summary.subject == original.summary.subject,
        "trash or move round trip returned an unexpected message"
    );
    Ok(restored)
}

pub(super) async fn batch(runtime: &Runtime, original: &[MailDetail]) -> anyhow::Result<()> {
    change_read_states(runtime, original, true).await?;
    let changed = verify_read_states(runtime, original, true).await;
    change_read_states(runtime, original, false).await?;
    verify_read_states(runtime, original, false).await?;
    changed
}

async fn change_read_states(
    runtime: &Runtime,
    original: &[MailDetail],
    toggle: bool,
) -> anyhow::Result<()> {
    let data = required(
        runtime
            .mail_batch(MailBatchInput {
                items: original
                    .iter()
                    .map(|mail| MailBatchItem {
                        mail_ref: mail.summary.mail_ref.clone(),
                        idempotency_key: operation_id(),
                        action: MailAction::MarkRead { is_read: mail.summary.is_read ^ toggle },
                    })
                    .collect(),
            })
            .await,
        "mail_batch fixtures",
    )?;
    anyhow::ensure!(data.items.len() == original.len(), "batch lost an entry");
    for entry in data.items {
        anyhow::ensure!(
            !entry.skipped
                && entry.error.is_none()
                && entry.result.is_some_and(|result| result.status == OperationState::Succeeded),
            "batch entry was not confirmed; stop without retry or cleanup: {}",
            entry.operation_id
        );
    }
    Ok(())
}

async fn verify_read_states(
    runtime: &Runtime,
    original: &[MailDetail],
    toggle: bool,
) -> anyhow::Result<()> {
    for original in original {
        let fetched = get(runtime, &original.summary.mail_ref).await?;
        anyhow::ensure!(
            fetched.summary.subject == original.summary.subject
                && fetched.summary.is_read == (original.summary.is_read ^ toggle),
            "batch read-state round trip was not verified"
        );
    }
    Ok(())
}

fn confirmed(
    response: ApiResponse<MailMutationResult>,
    operation: &str,
) -> anyhow::Result<MailMutationResult> {
    let result = required(response, operation)?;
    anyhow::ensure!(
        result.status == OperationState::Succeeded,
        "{operation} was not confirmed; stop without retry or cleanup: {}",
        result.operation_id
    );
    Ok(result)
}

fn reference(result: MailMutationResult) -> anyhow::Result<String> {
    result.mail_ref.ok_or_else(|| anyhow::anyhow!("confirmed move omitted its new reference"))
}
