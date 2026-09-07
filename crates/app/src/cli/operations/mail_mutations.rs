use super::input::{self, ensure_flag_mode, idempotency_key, read_write_json, required};
use super::mail_mutation_args::{
    BatchArgs, CategoriesArgs, DeleteArgs, FlagArg, FlagArgs, GetManyArgs, MoveArgs,
};
use super::output::{self, OutputKind, OutputMode};
use super::writes::approve;
use crate::cli::CliExit;
use crate::model::{
    ApiResponse, MailAction, MailBatchItem, MailDeleteInput, MailFlagState, MailGetManyInput,
    MailMoveInput, MailSetCategoriesInput, MailSetFlagInput, OperationState,
};
use crate::runtime::write_preview::PreparedWrite;
use crate::{Result, Runtime};

pub(super) async fn move_mail(
    runtime: &Runtime,
    args: MoveArgs,
    mode: OutputMode,
) -> Result<CliExit> {
    ensure_flag_mode(
        args.source.input.as_ref(),
        args.mail_ref.is_some() || args.destination_folder_id.is_some(),
    )?;
    let value: MailMoveInput = match args.source.input {
        Some(path) => read_write_json(&path, &args.control)?,
        None => MailMoveInput {
            mail_ref: required(args.mail_ref, "mail_ref")?,
            destination_folder_id: required(args.destination_folder_id, "destination_folder_id")?,
            idempotency_key: idempotency_key(&args.control),
        },
    };
    execute(
        runtime,
        MailBatchItem {
            mail_ref: value.mail_ref,
            idempotency_key: value.idempotency_key,
            action: MailAction::Move { destination_folder_id: value.destination_folder_id },
        },
        args.control.yes,
        false,
        mode,
    )
    .await
}
pub(super) async fn delete(
    runtime: &Runtime,
    args: DeleteArgs,
    mode: OutputMode,
) -> Result<CliExit> {
    ensure_flag_mode(args.source.input.as_ref(), args.mail_ref.is_some())?;
    let value: MailDeleteInput = match args.source.input {
        Some(path) => read_write_json(&path, &args.control)?,
        None => MailDeleteInput {
            mail_ref: required(args.mail_ref, "mail_ref")?,
            idempotency_key: idempotency_key(&args.control),
        },
    };
    execute(
        runtime,
        MailBatchItem {
            mail_ref: value.mail_ref,
            idempotency_key: value.idempotency_key,
            action: MailAction::Delete,
        },
        args.control.yes,
        false,
        mode,
    )
    .await
}
pub(super) async fn flag(runtime: &Runtime, args: FlagArgs, mode: OutputMode) -> Result<CliExit> {
    ensure_flag_mode(args.source.input.as_ref(), args.mail_ref.is_some() || args.flag.is_some())?;
    let value: MailSetFlagInput = match args.source.input {
        Some(path) => read_write_json(&path, &args.control)?,
        None => MailSetFlagInput {
            mail_ref: required(args.mail_ref, "mail_ref")?,
            idempotency_key: idempotency_key(&args.control),
            flag: match args.flag.ok_or_else(|| input::invalid("flag is required"))? {
                FlagArg::None => MailFlagState::None,
                FlagArg::Active => MailFlagState::Active,
                FlagArg::Complete => MailFlagState::Complete,
            },
        },
    };
    execute(
        runtime,
        MailBatchItem {
            mail_ref: value.mail_ref,
            idempotency_key: value.idempotency_key,
            action: MailAction::SetFlag { flag: value.flag },
        },
        args.control.yes,
        args.sync_folder,
        mode,
    )
    .await
}
pub(super) async fn categories(
    runtime: &Runtime,
    args: CategoriesArgs,
    mode: OutputMode,
) -> Result<CliExit> {
    ensure_flag_mode(
        args.source.input.as_ref(),
        args.mail_ref.is_some() || !args.categories.is_empty() || args.clear,
    )?;
    let value: MailSetCategoriesInput = match args.source.input {
        Some(path) => read_write_json(&path, &args.control)?,
        None => {
            if args.categories.is_empty() && !args.clear {
                return Err(input::invalid("specify --category or --clear"));
            }
            MailSetCategoriesInput {
                mail_ref: required(args.mail_ref, "mail_ref")?,
                idempotency_key: idempotency_key(&args.control),
                categories: args.categories,
            }
        }
    };
    execute(
        runtime,
        MailBatchItem {
            mail_ref: value.mail_ref,
            idempotency_key: value.idempotency_key,
            action: MailAction::SetCategories { categories: value.categories },
        },
        args.control.yes,
        args.sync_folder,
        mode,
    )
    .await
}

async fn execute(
    runtime: &Runtime,
    entry: MailBatchItem,
    yes: bool,
    sync_folder: bool,
    mode: OutputMode,
) -> Result<CliExit> {
    let mut prepared = runtime.prepare_cli_mail_mutation(&entry).await?;
    if sync_folder && matches!(prepared, PreparedWrite::Ready(_)) {
        runtime.sync_cli_mail_folders(std::slice::from_ref(&entry.mail_ref)).await?;
        prepared = runtime.prepare_cli_mail_mutation(&entry).await?;
    }
    let response = match prepared {
        PreparedWrite::Replay(result) => ApiResponse::success(result, Vec::new()),
        PreparedWrite::Ready(preview) => {
            if !matches!(entry.action, MailAction::Move { .. } | MailAction::Delete) {
                runtime.check_cli_mail_property(&entry.mail_ref).await?;
            }
            let Some(fingerprint) = approve(&preview, yes)? else {
                return Ok(CliExit::Declined);
            };
            runtime.mail_mutation(entry, Some(&fingerprint)).await
        }
    };
    let succeeded =
        response.data.as_ref().is_some_and(|result| result.status == OperationState::Succeeded);
    output::emit(response, mode, OutputKind::Write, succeeded)
}

pub(super) async fn batch(runtime: &Runtime, args: BatchArgs, mode: OutputMode) -> Result<CliExit> {
    let value: crate::MailBatchInput = input::read_json(&args.input)?;
    let mut preview = runtime.prepare_cli_mail_batch(&value).await?;
    let mut references = Vec::new();
    for entry in &value.items {
        if !matches!(entry.action, MailAction::Move { .. } | MailAction::Delete)
            && matches!(runtime.prepare_cli_mail_mutation(entry).await?, PreparedWrite::Ready(_))
        {
            references.push(entry.mail_ref.clone());
        }
    }
    if args.sync_folder && !references.is_empty() {
        runtime.sync_cli_mail_folders(&references).await?;
        preview = runtime.prepare_cli_mail_batch(&value).await?;
    }
    for reference in references {
        runtime.check_cli_mail_property(&reference).await?;
    }
    let Some(fingerprint) = approve(&preview, args.yes)? else {
        return Ok(CliExit::Declined);
    };
    let response = runtime.commit_cli_mail_batch(value, &fingerprint).await;
    let succeeded = response.data.as_ref().is_some_and(|value| {
        value.items.iter().all(|item| {
            item.error.is_none()
                && item
                    .result
                    .as_ref()
                    .is_some_and(|result| result.status == OperationState::Succeeded)
        })
    });
    output::emit(response, mode, OutputKind::Bulk, succeeded)
}

pub(super) async fn get_many(
    runtime: &Runtime,
    args: GetManyArgs,
    mode: OutputMode,
) -> Result<CliExit> {
    ensure_flag_mode(
        args.source.input.as_ref(),
        !args.mail_refs.is_empty() || args.body_limit.is_some() || args.total_body_limit.is_some(),
    )?;
    let value = match args.source.input {
        Some(path) => input::read_json(&path)?,
        None => MailGetManyInput {
            mail_refs: args.mail_refs,
            body_limit: args.body_limit,
            total_body_limit: args.total_body_limit,
        },
    };
    let response = runtime.mail_get_many(value).await;
    let succeeded = response
        .data
        .as_ref()
        .is_some_and(|value| value.items.iter().all(|item| item.error.is_none()));
    output::emit(response, mode, OutputKind::Bulk, succeeded)
}
