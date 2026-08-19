use std::process::{Command, Output, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt as _;

use super::ClientKind;
use crate::{AppError, ErrorCode, Result};

const CLIENT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn replace_cli_server(executable: &str, remove: &[&str], add: &[&str]) -> Result<()> {
    command(executable, remove, true)?;
    command(executable, add, false).map(|_| ())
}

pub(super) fn command(executable: &str, arguments: &[&str], allow_failure: bool) -> Result<bool> {
    let output = output_with_timeout(executable, arguments, CLIENT_COMMAND_TIMEOUT)?;
    let success = output.status.success();
    if success || allow_failure {
        Ok(success)
    } else {
        Err(AppError::new(ErrorCode::ConfigInvalid, "AI client rejected MCP configuration"))
    }
}

pub(super) fn detect_version(executable: &str) -> Option<String> {
    let output = output_with_timeout(executable, &["--version"], CLIENT_COMMAND_TIMEOUT).ok()?;
    if !output.status.success() {
        return None;
    }
    [output.stdout, output.stderr]
        .into_iter()
        .flat_map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .lines()
                .map(str::trim)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .find(|line| !line.is_empty())
}

pub(super) fn output_with_timeout(
    executable: &str,
    arguments: &[&str],
    timeout: Duration,
) -> Result<Output> {
    let mut child = Command::new(executable)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| AppError::new(ErrorCode::NotFound, "AI client executable is unavailable"))?;
    let status = child
        .wait_timeout(timeout)
        .map_err(|_| AppError::new(ErrorCode::ConfigInvalid, "cannot monitor AI client command"))?;
    if status.is_none() {
        drop(child.kill());
        drop(child.wait());
        return Err(AppError::new(ErrorCode::ConfigInvalid, "AI client command timed out"));
    }
    child.wait_with_output().map_err(|_| {
        AppError::new(ErrorCode::ConfigInvalid, "cannot read AI client command output")
    })
}

pub(super) const fn client_name(client: ClientKind) -> &'static str {
    match client {
        ClientKind::Codex => "codex",
        ClientKind::Claude => "claude",
        ClientKind::Opencode => "opencode",
    }
}
