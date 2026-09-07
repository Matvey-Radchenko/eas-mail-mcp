use super::input::{
    body, ensure_flag_mode, idempotency_key, read_json, read_write_json, required, selected,
};
use super::mail_args::{
    AttachmentReferenceArgs, MailForwardArgs, MailGetArgs, MailListArgs, MailMarkReadArgs,
    MailReferenceArgs, MailReplyArgs, MailSearchArgs, MailSendArgs,
};
use crate::model::{
    AttachmentDownloadInput, MailAttachmentsInput, MailForwardInput, MailGetInput, MailListInput,
    MailReplyInput, MailSearchInput, MailSendInput, MarkReadInput, OutgoingAttachmentInput,
};
use crate::{AppError, ErrorCode, Result};

const DEFAULT_RESULTS: usize = 50;
const MAX_RESULTS: usize = 10_000;

pub(super) struct PagedInput<T> {
    pub(super) input: T,
    pub(super) maximum: Option<usize>,
}

pub(super) fn list(arguments: MailListArgs) -> Result<PagedInput<MailListInput>> {
    let has_flags = !arguments.accounts.is_empty()
        || !arguments.folders.is_empty()
        || arguments.limit.is_some()
        || arguments.all;
    ensure_flag_mode(arguments.source.input.as_ref(), has_flags)?;
    if let Some(path) = arguments.source.input {
        let input: MailListInput = read_json(&path)?;
        reject_cursor(input.cursor.as_deref())?;
        let maximum = Some(input.limit.map_or(DEFAULT_RESULTS, usize::from));
        return Ok(PagedInput { input, maximum });
    }
    let maximum = total_limit(arguments.limit, arguments.all)?;
    Ok(PagedInput {
        input: MailListInput {
            account_ids: selected(arguments.accounts),
            folder_ids: selected(arguments.folders),
            cursor: None,
            limit: page_limit(maximum),
        },
        maximum,
    })
}

pub(super) fn search(arguments: MailSearchArgs) -> Result<PagedInput<MailSearchInput>> {
    let filters = search_filters(arguments.filters);
    let has_flags = arguments.query.is_some()
        || !arguments.accounts.is_empty()
        || arguments.limit.is_some()
        || arguments.all
        || filters.from.is_some()
        || filters.to.is_some()
        || filters.received_after.is_some()
        || filters.received_before.is_some()
        || filters.is_read.is_some()
        || filters.has_attachments.is_some()
        || !filters.folder_ids.is_empty();
    ensure_flag_mode(arguments.source.input.as_ref(), has_flags)?;
    if let Some(path) = arguments.source.input {
        let input: MailSearchInput = read_json(&path)?;
        reject_cursor(input.cursor.as_deref())?;
        let maximum = Some(input.limit.map_or(DEFAULT_RESULTS, usize::from));
        return Ok(PagedInput { input, maximum });
    }
    let maximum = total_limit(arguments.limit, arguments.all)?;
    Ok(PagedInput {
        input: MailSearchInput {
            filters,
            query: arguments.query.unwrap_or_default(),
            account_ids: selected(arguments.accounts),
            cursor: None,
            limit: page_limit(maximum),
        },
        maximum,
    })
}

fn search_filters(input: super::mail_args::MailSearchFilterArgs) -> crate::MailSearchFilters {
    crate::MailSearchFilters {
        from: input.from,
        to: input.to,
        received_after: input.received_after,
        received_before: input.received_before,
        is_read: input.is_read,
        has_attachments: input.has_attachments,
        folder_ids: input.folders,
    }
}

pub(super) fn thread(input: super::mail_args::MailThreadArgs) -> Result<crate::MailGetThreadInput> {
    ensure_flag_mode(
        input.source.input.as_ref(),
        input.mail_ref.is_some()
            || input.limit.is_some()
            || input.body_limit.is_some()
            || input.total_body_limit.is_some(),
    )?;
    input.source.input.map_or_else(
        || {
            Ok(crate::MailGetThreadInput {
                mail_ref: required(input.mail_ref, "mail_ref")?,
                limit: input.limit,
                body_limit: input.body_limit,
                total_body_limit: input.total_body_limit,
            })
        },
        |path| read_json(&path),
    )
}

pub(super) fn get(arguments: MailGetArgs) -> Result<MailGetInput> {
    ensure_flag_mode(
        arguments.source.input.as_ref(),
        arguments.mail_ref.is_some() || arguments.body_limit.is_some(),
    )?;
    arguments.source.input.map_or_else(
        || {
            Ok(MailGetInput {
                mail_ref: required(arguments.mail_ref, "mail_ref")?,
                body_limit: arguments.body_limit,
            })
        },
        |path| read_json(&path),
    )
}

pub(super) fn attachments(arguments: MailReferenceArgs) -> Result<MailAttachmentsInput> {
    ensure_flag_mode(arguments.source.input.as_ref(), arguments.mail_ref.is_some())?;
    arguments.source.input.map_or_else(
        || Ok(MailAttachmentsInput { mail_ref: required(arguments.mail_ref, "mail_ref")? }),
        |path| read_json(&path),
    )
}

pub(super) fn download(arguments: AttachmentReferenceArgs) -> Result<AttachmentDownloadInput> {
    ensure_flag_mode(arguments.source.input.as_ref(), arguments.attachment_ref.is_some())?;
    arguments.source.input.map_or_else(
        || {
            Ok(AttachmentDownloadInput {
                attachment_ref: required(arguments.attachment_ref, "attachment_ref")?,
            })
        },
        |path| read_json(&path),
    )
}

pub(super) fn mark_read(arguments: MailMarkReadArgs) -> Result<(MarkReadInput, bool)> {
    let has_flags = arguments.mail_ref.is_some()
        || arguments.state.is_some()
        || arguments.control.idempotency_key.is_some();
    ensure_flag_mode(arguments.source.input.as_ref(), has_flags)?;
    let input = if let Some(path) = arguments.source.input {
        read_write_json(&path, &arguments.control)?
    } else {
        MarkReadInput {
            mail_ref: required(arguments.mail_ref, "mail_ref")?,
            is_read: matches!(
                arguments.state.ok_or_else(|| missing("state"))?,
                super::common::ReadStateArg::Read
            ),
            idempotency_key: idempotency_key(&arguments.control),
        }
    };
    Ok((input, arguments.control.yes))
}

pub(super) fn send(arguments: MailSendArgs) -> Result<(MailSendInput, bool)> {
    let has_flags = arguments.account.is_some()
        || !arguments.to.is_empty()
        || !arguments.cc.is_empty()
        || !arguments.bcc.is_empty()
        || arguments.subject.is_some()
        || !arguments.attachments.is_empty()
        || body_flags(&arguments.content)
        || arguments.control.idempotency_key.is_some();
    ensure_flag_mode(arguments.source.input.as_ref(), has_flags)?;
    let input = if let Some(path) = arguments.source.input {
        read_write_json(&path, &arguments.control)?
    } else {
        MailSendInput {
            account_id: required(arguments.account, "account")?,
            to: arguments.to,
            cc: arguments.cc,
            bcc: arguments.bcc,
            subject: required(arguments.subject, "subject")?,
            body: body(&arguments.content)?,
            attachments: local_attachments(arguments.attachments)?,
            idempotency_key: idempotency_key(&arguments.control),
        }
    };
    Ok((input, arguments.control.yes))
}

pub(super) fn reply(arguments: MailReplyArgs) -> Result<(MailReplyInput, bool)> {
    let has_flags = arguments.mail_ref.is_some()
        || !arguments.attachments.is_empty()
        || body_flags(&arguments.content)
        || arguments.reply_all
        || arguments.control.idempotency_key.is_some();
    ensure_flag_mode(arguments.source.input.as_ref(), has_flags)?;
    let input = if let Some(path) = arguments.source.input {
        read_write_json(&path, &arguments.control)?
    } else {
        MailReplyInput {
            mail_ref: required(arguments.mail_ref, "mail_ref")?,
            body: body(&arguments.content)?,
            attachments: local_attachments(arguments.attachments)?,
            reply_all: arguments.reply_all,
            idempotency_key: idempotency_key(&arguments.control),
        }
    };
    Ok((input, arguments.control.yes))
}

pub(super) fn forward(arguments: MailForwardArgs) -> Result<(MailForwardInput, bool)> {
    let has_flags = arguments.mail_ref.is_some()
        || !arguments.to.is_empty()
        || !arguments.cc.is_empty()
        || !arguments.bcc.is_empty()
        || !arguments.attachments.is_empty()
        || body_flags(&arguments.content)
        || arguments.control.idempotency_key.is_some();
    ensure_flag_mode(arguments.source.input.as_ref(), has_flags)?;
    let input = if let Some(path) = arguments.source.input {
        read_write_json(&path, &arguments.control)?
    } else {
        MailForwardInput {
            mail_ref: required(arguments.mail_ref, "mail_ref")?,
            to: arguments.to,
            cc: arguments.cc,
            bcc: arguments.bcc,
            body: body(&arguments.content)?,
            attachments: local_attachments(arguments.attachments)?,
            idempotency_key: idempotency_key(&arguments.control),
        }
    };
    Ok((input, arguments.control.yes))
}

fn total_limit(limit: Option<usize>, all: bool) -> Result<Option<usize>> {
    if all {
        return Ok(None);
    }
    let value = limit.unwrap_or(DEFAULT_RESULTS);
    if value == 0 || value > MAX_RESULTS {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            format!("limit must be between 1 and {MAX_RESULTS}"),
        ));
    }
    Ok(Some(value))
}

fn page_limit(maximum: Option<usize>) -> Option<u8> {
    Some(maximum.unwrap_or(100).min(100) as u8)
}

fn reject_cursor(cursor: Option<&str>) -> Result<()> {
    if cursor.is_some() {
        Err(AppError::new(
            ErrorCode::ValidationFailed,
            "CLI input cannot use a process-local MCP cursor; use --limit or --all",
        ))
    } else {
        Ok(())
    }
}

fn body_flags(source: &super::common::BodySource) -> bool {
    source.body.is_some() || source.body_file.is_some() || source.body_stdin
}

fn missing(label: &'static str) -> AppError {
    AppError::new(
        ErrorCode::ValidationFailed,
        format!("{label} is required unless --input is used"),
    )
}

fn local_attachments(paths: Vec<std::path::PathBuf>) -> Result<Vec<OutgoingAttachmentInput>> {
    paths
        .into_iter()
        .map(|path| {
            let path = std::path::absolute(path).map_err(|_| {
                AppError::new(ErrorCode::ValidationFailed, "cannot resolve attachment path")
            })?;
            let path = path.into_os_string().into_string().map_err(|_| {
                AppError::new(ErrorCode::ValidationFailed, "attachment path must be UTF-8")
            })?;
            Ok(OutgoingAttachmentInput { path, filename: None, content_type: None })
        })
        .collect()
}

#[cfg(test)]
#[path = "mail_input_tests.rs"]
mod tests;
