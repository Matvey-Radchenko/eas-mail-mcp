mod configuration;
mod files;
mod process;

use clap::ValueEnum;
use serde_json::Value;

use self::configuration::{
    configure_claude, configure_codex, configure_opencode, unconfigure_claude, unconfigure_cli,
    unconfigure_opencode,
};
use self::files::ClientFiles;
use self::process::{client_name, detect_version};
use super::{ClientArgs, ClientCommand, confirm};
use crate::{AppError, ErrorCode, Paths, Result};

const SERVER: &str = "eas-mail";
const WRITE_TOOLS: [&str; 4] = ["mail_mark_read", "mail_send", "mail_reply", "mail_forward"];

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum ClientKind {
    Codex,
    Claude,
    Opencode,
}

pub(super) fn run(paths: &Paths, command: ClientCommand) -> Result<Value> {
    match command {
        ClientCommand::Configure(arguments) => configure(paths, arguments),
        ClientCommand::Unconfigure(arguments) => unconfigure(paths, arguments),
    }
}

pub(super) fn configure_detected(paths: &Paths) -> Result<Vec<Value>> {
    let mut results = Vec::new();
    for client in [ClientKind::Codex, ClientKind::Claude, ClientKind::Opencode] {
        let executable = client_name(client).to_owned();
        let Some(version) = detect_version(&executable) else { continue };
        if confirm(&format!("Configure {}", client_name(client)))? {
            results.push(configure(paths, ClientArgs { client, executable: Some(executable) })?);
        } else {
            results.push(serde_json::json!({
                "client": client_name(client),
                "version": version,
                "configured": false,
                "reason": "declined",
            }));
        }
    }
    Ok(results)
}

fn configure(paths: &Paths, arguments: ClientArgs) -> Result<Value> {
    let executable = arguments.executable.unwrap_or_else(|| client_name(arguments.client).into());
    let version = detect_version(&executable);
    let bridge = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|_| AppError::new(ErrorCode::ConfigInvalid, "cannot resolve MCP executable"))?;
    let files = ClientFiles::discover()?;
    let backups = match arguments.client {
        ClientKind::Codex => configure_codex(paths, &files.codex, &executable, &bridge)?,
        ClientKind::Claude => configure_claude(paths, &files, &executable, &bridge)?,
        ClientKind::Opencode => configure_opencode(paths, &files.opencode, &bridge)?,
    };
    Ok(serde_json::json!({
        "client": client_name(arguments.client),
        "version": version,
        "configured": true,
        "write_execution": "direct_when_account_enabled",
        "backups": backups,
    }))
}

fn unconfigure(paths: &Paths, arguments: ClientArgs) -> Result<Value> {
    let executable = arguments.executable.unwrap_or_else(|| client_name(arguments.client).into());
    let version = detect_version(&executable);
    let files = ClientFiles::discover()?;
    let backups = match arguments.client {
        ClientKind::Codex => unconfigure_cli(paths, &executable, files.codex, false)?,
        ClientKind::Claude => unconfigure_claude(paths, &files, &executable)?,
        ClientKind::Opencode => unconfigure_opencode(paths, &files.opencode)?,
    };
    Ok(serde_json::json!({
        "client": client_name(arguments.client),
        "version": version,
        "configured": false,
        "backups": backups,
    }))
}

#[cfg(test)]
use self::configuration::remove_codex_generated_approvals;
#[cfg(test)]
use self::files::{
    array_entry, backup, object_entry, path_text, paths_to_strings, read_json, restore, write_json,
};
#[cfg(test)]
use self::process::{command, output_with_timeout, replace_cli_server};

#[cfg(test)]
mod tests;
