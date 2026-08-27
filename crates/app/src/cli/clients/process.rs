#[cfg(windows)]
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

#[cfg(not(windows))]
use wait_timeout::ChildExt as _;

use super::ClientKind;
use crate::{AppError, ErrorCode, Result};

const CLIENT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(windows)]
mod windows_job;

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
    let executable = resolve_executable(executable);
    let mut command = client_command(&executable, arguments);
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    command_output(command, timeout)
}

#[cfg(windows)]
fn command_output(command: Command, timeout: Duration) -> Result<Output> {
    windows_job::output(command, timeout)
}

#[cfg(not(windows))]
fn command_output(mut command: Command, timeout: Duration) -> Result<Output> {
    let mut child = command
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

#[cfg(windows)]
fn client_command(executable: &Path, arguments: &[&str]) -> Command {
    use std::os::windows::process::CommandExt as _;

    if executable.extension().and_then(|value| value.to_str()).is_some_and(|extension| {
        extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
    }) {
        let shell = std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into());
        let mut command = Command::new(shell);
        command.args(["/D", "/S", "/C"]);
        command.raw_arg(batch_command_line(executable, arguments));
        return command;
    }
    let mut command = Command::new(executable);
    command.args(arguments);
    command
}

#[cfg(windows)]
fn batch_command_line(executable: &Path, arguments: &[&str]) -> OsString {
    let mut line = OsString::from("\"");
    push_quoted_batch_argument(&mut line, executable.as_os_str());
    for argument in arguments {
        line.push(" ");
        push_quoted_batch_argument(&mut line, OsStr::new(argument));
    }
    line.push("\"");
    line
}

#[cfg(windows)]
fn push_quoted_batch_argument(line: &mut OsString, argument: &OsStr) {
    line.push("\"");
    line.push(argument);
    line.push("\"");
}

#[cfg(not(windows))]
fn client_command(executable: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new(executable);
    command.args(arguments);
    command
}

#[cfg(windows)]
pub(super) fn resolve_executable(executable: &str) -> PathBuf {
    let requested = Path::new(executable);
    if requested.extension().is_some() {
        return requested.to_owned();
    }
    let extensions = windows_executable_extensions();
    if requested.components().count() > 1 {
        return with_first_existing_extension(requested, &extensions)
            .unwrap_or_else(|| requested.to_owned());
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .find_map(|directory| {
            with_first_existing_extension(&directory.join(requested), &extensions)
        })
        .unwrap_or_else(|| requested.to_owned())
}

#[cfg(not(windows))]
pub(super) fn resolve_executable(executable: &str) -> PathBuf {
    PathBuf::from(executable)
}

#[cfg(windows)]
fn windows_executable_extensions() -> Vec<String> {
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

#[cfg(windows)]
fn with_first_existing_extension(path: &Path, extensions: &[String]) -> Option<PathBuf> {
    extensions.iter().find_map(|extension| {
        let mut candidate = path.as_os_str().to_owned();
        candidate.push(extension);
        let candidate = PathBuf::from(candidate);
        candidate.is_file().then_some(candidate)
    })
}

pub(super) const fn client_name(client: ClientKind) -> &'static str {
    match client {
        ClientKind::Codex => "codex",
        ClientKind::Claude => "claude",
        ClientKind::Opencode => "opencode",
    }
}

pub(super) const fn client_display_name(client: ClientKind) -> &'static str {
    match client {
        ClientKind::Codex => "Codex",
        ClientKind::Claude => "Claude Code",
        ClientKind::Opencode => "OpenCode",
    }
}
