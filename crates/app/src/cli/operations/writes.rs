use std::io::Write as _;

use super::output::{self, OutputKind, OutputMode};
use crate::cli::CliExit;
use crate::cli::terminal::confirm_controlling_tty;
use crate::model::{
    CalendarCancelInput, CalendarCreateInput, CalendarDeleteInput, CalendarOperationResult,
    CalendarOperationState, CalendarRespondInput, CalendarUpdateInput, MailForwardInput,
    MailReplyInput, MailSendInput, MarkReadInput, OperationResult, OperationState,
};
use crate::runtime::write_preview::{PreparedWrite, WritePreview};
use crate::{ApiResponse, AppError, ErrorCode, Runtime};

pub(super) async fn mail_mark_read(
    runtime: &Runtime,
    input: MarkReadInput,
    yes: bool,
    sync_folder: bool,
    mode: OutputMode,
) -> crate::Result<CliExit> {
    let mut prepared = runtime.prepare_cli_mail_mark_read(&input).await?;
    if sync_folder && matches!(prepared, PreparedWrite::Ready(_)) {
        runtime.sync_cli_mail_folders(std::slice::from_ref(&input.mail_ref)).await?;
        prepared = runtime.prepare_cli_mail_mark_read(&input).await?;
    }
    match prepared {
        PreparedWrite::Replay(result) => mail_result(success(result), mode),
        PreparedWrite::Ready(preview) => {
            runtime.check_cli_mail_property(&input.mail_ref).await?;
            let Some(fingerprint) = approve(&preview, yes)? else {
                return Ok(CliExit::Declined);
            };
            mail_result(runtime.commit_cli_mail_mark_read(input, &fingerprint).await, mode)
        }
    }
}

pub(super) async fn mail_send(
    runtime: &Runtime,
    input: MailSendInput,
    yes: bool,
    mode: OutputMode,
) -> crate::Result<CliExit> {
    match runtime.prepare_cli_mail_send(&input)? {
        PreparedWrite::Replay(result) => mail_result(success(result), mode),
        PreparedWrite::Ready(preview) => {
            let Some(fingerprint) = approve(&preview, yes)? else {
                return Ok(CliExit::Declined);
            };
            mail_result(runtime.commit_cli_mail_send(input, &fingerprint).await, mode)
        }
    }
}

pub(super) async fn mail_reply(
    runtime: &Runtime,
    input: MailReplyInput,
    yes: bool,
    mode: OutputMode,
) -> crate::Result<CliExit> {
    match runtime.prepare_cli_mail_reply(&input).await? {
        PreparedWrite::Replay(result) => mail_result(success(result), mode),
        PreparedWrite::Ready(preview) => {
            let Some(fingerprint) = approve(&preview, yes)? else {
                return Ok(CliExit::Declined);
            };
            mail_result(runtime.commit_cli_mail_reply(input, &fingerprint).await, mode)
        }
    }
}

pub(super) async fn mail_forward(
    runtime: &Runtime,
    input: MailForwardInput,
    yes: bool,
    mode: OutputMode,
) -> crate::Result<CliExit> {
    match runtime.prepare_cli_mail_forward(&input).await? {
        PreparedWrite::Replay(result) => mail_result(success(result), mode),
        PreparedWrite::Ready(preview) => {
            let Some(fingerprint) = approve(&preview, yes)? else {
                return Ok(CliExit::Declined);
            };
            mail_result(runtime.commit_cli_mail_forward(input, &fingerprint).await, mode)
        }
    }
}

pub(super) async fn calendar_create(
    runtime: &Runtime,
    input: CalendarCreateInput,
    yes: bool,
    mode: OutputMode,
) -> crate::Result<CliExit> {
    match runtime.prepare_cli_calendar_create(&input).await? {
        PreparedWrite::Replay(result) => calendar_result(success(result), mode),
        PreparedWrite::Ready(preview) => {
            let Some(fingerprint) = approve(&preview, yes)? else {
                return Ok(CliExit::Declined);
            };
            calendar_result(runtime.commit_cli_calendar_create(input, &fingerprint).await, mode)
        }
    }
}

pub(super) async fn calendar_update(
    runtime: &Runtime,
    input: CalendarUpdateInput,
    yes: bool,
    mode: OutputMode,
) -> crate::Result<CliExit> {
    match runtime.prepare_cli_calendar_update(&input).await? {
        PreparedWrite::Replay(result) => calendar_result(success(result), mode),
        PreparedWrite::Ready(preview) => {
            let Some(fingerprint) = approve(&preview, yes)? else {
                return Ok(CliExit::Declined);
            };
            calendar_result(runtime.commit_cli_calendar_update(input, &fingerprint).await, mode)
        }
    }
}

pub(super) async fn calendar_delete(
    runtime: &Runtime,
    input: CalendarDeleteInput,
    yes: bool,
    mode: OutputMode,
) -> crate::Result<CliExit> {
    match runtime.prepare_cli_calendar_delete(&input).await? {
        PreparedWrite::Replay(result) => calendar_result(success(result), mode),
        PreparedWrite::Ready(preview) => {
            let Some(fingerprint) = approve(&preview, yes)? else {
                return Ok(CliExit::Declined);
            };
            calendar_result(runtime.commit_cli_calendar_delete(input, &fingerprint).await, mode)
        }
    }
}

pub(super) async fn calendar_cancel(
    runtime: &Runtime,
    input: CalendarCancelInput,
    yes: bool,
    mode: OutputMode,
) -> crate::Result<CliExit> {
    match runtime.prepare_cli_calendar_cancel(&input).await? {
        PreparedWrite::Replay(result) => calendar_result(success(result), mode),
        PreparedWrite::Ready(preview) => {
            let Some(fingerprint) = approve(&preview, yes)? else {
                return Ok(CliExit::Declined);
            };
            calendar_result(runtime.commit_cli_calendar_cancel(input, &fingerprint).await, mode)
        }
    }
}

pub(super) async fn calendar_respond(
    runtime: &Runtime,
    input: CalendarRespondInput,
    yes: bool,
    mode: OutputMode,
) -> crate::Result<CliExit> {
    match runtime.prepare_cli_calendar_respond(&input).await? {
        PreparedWrite::Replay(result) => calendar_result(success(result), mode),
        PreparedWrite::Ready(preview) => {
            let Some(fingerprint) = approve(&preview, yes)? else {
                return Ok(CliExit::Declined);
            };
            calendar_result(runtime.commit_cli_calendar_respond(input, &fingerprint).await, mode)
        }
    }
}

pub(super) fn approve(preview: &WritePreview, yes: bool) -> crate::Result<Option<String>> {
    writeln!(std::io::stderr().lock(), "{}", preview.render())
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot write mutation preview"))?;
    let fingerprint = preview.fingerprint()?;
    if yes || confirm_controlling_tty("Execute this write")? {
        Ok(Some(fingerprint))
    } else {
        writeln!(std::io::stderr().lock(), "Write declined")
            .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot write CLI output"))?;
        Ok(None)
    }
}

fn mail_result(response: ApiResponse<OperationResult>, mode: OutputMode) -> crate::Result<CliExit> {
    let succeeded =
        response.data.as_ref().is_some_and(|value| value.status == OperationState::Succeeded);
    output::emit(response, mode, OutputKind::Write, succeeded)
}

fn calendar_result(
    response: ApiResponse<CalendarOperationResult>,
    mode: OutputMode,
) -> crate::Result<CliExit> {
    let succeeded = response
        .data
        .as_ref()
        .is_some_and(|value| value.status == CalendarOperationState::Succeeded);
    output::emit(response, mode, OutputKind::Write, succeeded)
}

fn success<T>(value: T) -> ApiResponse<T> {
    ApiResponse::success(value, Vec::new())
}
