use std::io::Write as _;

use serde::Serialize;

use super::human;
use crate::cli::CliExit;
use crate::{ApiResponse, AppError, ErrorCode, Result};

#[derive(Debug, Clone, Copy)]
pub(in crate::cli) enum OutputMode {
    Json,
    Human,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum OutputKind {
    Accounts,
    Folders,
    People,
    MailList,
    MailDetail,
    Attachments,
    Download,
    Availability,
    Slots,
    CalendarList,
    CalendarEvent,
    Write,
}

pub(super) fn emit<T: Serialize>(
    response: ApiResponse<T>,
    mode: OutputMode,
    kind: OutputKind,
    write_succeeded: bool,
) -> Result<CliExit> {
    if let Some(error) = response.error {
        return Err(AppError { envelope: error });
    }
    match mode {
        OutputMode::Json => emit_json(&response)?,
        OutputMode::Human => emit_human(&response, kind)?,
    }
    Ok(if write_succeeded { CliExit::Success } else { CliExit::WriteNotSucceeded })
}

fn emit_json<T: Serialize>(response: &ApiResponse<T>) -> Result<()> {
    let document = serde_json::to_string_pretty(response)
        .map_err(|_| AppError::new(ErrorCode::ProtocolError, "cannot serialize CLI output"))?;
    writeln!(std::io::stdout().lock(), "{document}")
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot write CLI output"))
}

fn emit_human<T: Serialize>(response: &ApiResponse<T>, kind: OutputKind) -> Result<()> {
    for warning in &response.warnings {
        writeln!(
            std::io::stderr().lock(),
            "warning [{}] {}: {}",
            warning.code,
            literal(&warning.account_id),
            literal(&warning.message)
        )
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot write CLI warning"))?;
    }
    let value = response.data.as_ref().ok_or_else(|| {
        AppError::new(ErrorCode::ProtocolError, "successful CLI response has no data")
    })?;
    let value = serde_json::to_value(value)
        .map_err(|_| AppError::new(ErrorCode::ProtocolError, "cannot serialize CLI output"))?;
    let document = human::render(&value, kind)?;
    writeln!(std::io::stdout().lock(), "{document}")
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot write CLI output"))
}

fn literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"unavailable\"".to_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn warning_literals_escape_lines_and_terminal_controls() {
        assert_eq!(super::literal("line\n\u{1b}[31m"), "\"line\\n\\u001b[31m\"");
    }
}
