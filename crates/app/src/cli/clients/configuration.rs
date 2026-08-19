use std::path::{Path, PathBuf};

use serde_json::Value;
use toml_edit::{DocumentMut, Item};

use super::files::{
    ClientFiles, backup, exists, object_entry, path_text, paths_to_strings, read_json, read_text,
    restore, write_json, write_private,
};
use super::process::{command, replace_cli_server};
use super::{SERVER, WRITE_TOOLS};
use crate::{AppError, ErrorCode, Paths, Result};

pub(super) fn configure_codex(
    paths: &Paths,
    config: &Path,
    executable: &str,
    bridge: &Path,
) -> Result<Vec<String>> {
    let backup = backup(paths, config, "codex")?;
    let result = (|| {
        replace_cli_server(
            executable,
            &["mcp", "remove", SERVER],
            &["mcp", "add", SERVER, "--", path_text(bridge)?, "serve"],
        )?;
        remove_codex_generated_approvals(config)?;
        command(executable, &["mcp", "get", SERVER], false).map(|_| ())
    })();
    if let Err(error) = result {
        restore(config, backup.as_deref())?;
        return Err(error);
    }
    Ok(paths_to_strings(backup.into_iter()))
}

pub(super) fn remove_codex_generated_approvals(path: &Path) -> Result<()> {
    if !exists(path) {
        return Ok(());
    }
    let content = read_text(path)?;
    let mut document = content
        .parse::<DocumentMut>()
        .map_err(|_| AppError::new(ErrorCode::ConfigInvalid, "Codex configuration is invalid"))?;
    let mut changed = false;
    {
        let Some(servers) = document.get_mut("mcp_servers").and_then(Item::as_table_mut) else {
            return Ok(());
        };
        let Some(server) = servers.get_mut(SERVER).and_then(Item::as_table_mut) else {
            return Ok(());
        };
        let remove_tools = {
            let Some(tools) = server.get_mut("tools").and_then(Item::as_table_mut) else {
                return Ok(());
            };
            for tool in WRITE_TOOLS {
                let remove_tool =
                    tools.get_mut(tool).and_then(Item::as_table_mut).is_some_and(|settings| {
                        let generated = settings
                            .get("approval_mode")
                            .and_then(Item::as_str)
                            .is_some_and(|value| matches!(value, "prompt" | "approve"));
                        if generated {
                            settings.remove("approval_mode");
                            changed = true;
                        }
                        settings.is_empty()
                    });
                if remove_tool {
                    tools.remove(tool);
                    changed = true;
                }
            }
            tools.is_empty()
        };
        if remove_tools {
            server.remove("tools");
            changed = true;
        }
    }
    if changed {
        write_private(path, document.to_string().as_bytes(), "Codex configuration")?;
    }
    Ok(())
}

pub(super) fn configure_claude(
    paths: &Paths,
    files: &ClientFiles,
    executable: &str,
    bridge: &Path,
) -> Result<Vec<String>> {
    let mcp_config = &files.claude_mcp;
    let settings = &files.claude_settings;
    let mcp_backup = backup(paths, mcp_config, "claude-mcp")?;
    let settings_backup = backup(paths, settings, "claude-settings")?;
    let result = (|| {
        replace_cli_server(
            executable,
            &["mcp", "remove", "--scope", "user", SERVER],
            &["mcp", "add", "--scope", "user", SERVER, "--", path_text(bridge)?, "serve"],
        )?;
        if exists(settings) {
            let mut document = read_json(settings, false)?;
            remove_claude_write_rules(&mut document);
            write_json(settings, &document)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mcp_restore = restore(mcp_config, mcp_backup.as_deref());
        let settings_restore = restore(settings, settings_backup.as_deref());
        mcp_restore?;
        settings_restore?;
        return Err(error);
    }
    Ok(paths_to_strings(mcp_backup.into_iter().chain(settings_backup)))
}

pub(super) fn configure_opencode(
    paths: &Paths,
    config: &Path,
    bridge: &Path,
) -> Result<Vec<String>> {
    let mut document = read_json(config, true)?;
    let backup = backup(paths, config, "opencode-1")?;
    let result = (|| {
        let mcp = object_entry(&mut document, "mcp")?;
        mcp.insert(
            SERVER.into(),
            serde_json::json!({
                "type": "local",
                "command": [path_text(bridge)?, "serve"],
                "enabled": true,
            }),
        );
        if let Some(permissions) = document.get_mut("permission").and_then(Value::as_object_mut) {
            remove_opencode_write_rules(permissions);
        }
        write_json(config, &document)
    })();
    if let Err(error) = result {
        restore(config, backup.as_deref())?;
        return Err(error);
    }
    Ok(paths_to_strings(backup.into_iter()))
}

pub(super) fn unconfigure_cli(
    paths: &Paths,
    executable: &str,
    config: PathBuf,
    claude: bool,
) -> Result<Vec<String>> {
    let backup = backup(paths, &config, "client-remove")?;
    let args = if claude {
        vec!["mcp", "remove", "--scope", "user", SERVER]
    } else {
        vec!["mcp", "remove", SERVER]
    };
    let result = command(executable, &args, true);
    if let Err(error) = result {
        restore(&config, backup.as_deref())?;
        return Err(error);
    }
    Ok(paths_to_strings(backup.into_iter()))
}

pub(super) fn unconfigure_claude(
    paths: &Paths,
    files: &ClientFiles,
    executable: &str,
) -> Result<Vec<String>> {
    let settings = &files.claude_settings;
    let mut backups = unconfigure_cli(paths, executable, files.claude_mcp.clone(), true)?;
    let backup = backup(paths, settings, "claude-settings-remove")?;
    if exists(settings) {
        let mut document = read_json(settings, false)?;
        remove_claude_write_rules(&mut document);
        write_json(settings, &document)?;
    }
    backups.extend(paths_to_strings(backup.into_iter()));
    Ok(backups)
}

pub(super) fn unconfigure_opencode(paths: &Paths, config: &Path) -> Result<Vec<String>> {
    if !exists(config) {
        return Ok(Vec::new());
    }
    let mut document = read_json(config, true)?;
    let backup = backup(paths, config, "opencode-1-remove")?;
    if let Some(mcp) = document.get_mut("mcp").and_then(Value::as_object_mut) {
        mcp.remove(SERVER);
    }
    if let Some(permissions) = document.get_mut("permission").and_then(Value::as_object_mut) {
        remove_opencode_write_rules(permissions);
    }
    if let Err(error) = write_json(config, &document) {
        restore(config, backup.as_deref())?;
        return Err(error);
    }
    Ok(paths_to_strings(backup.into_iter()))
}

fn remove_claude_write_rules(document: &mut Value) {
    if let Some(permissions) = document.get_mut("permissions").and_then(Value::as_object_mut) {
        for key in ["ask", "allow"] {
            if let Some(rules) = permissions.get_mut(key).and_then(Value::as_array_mut) {
                rules.retain(|rule| !is_write_rule(rule));
            }
        }
    }
}

fn remove_opencode_write_rules(permissions: &mut serde_json::Map<String, Value>) {
    for tool in WRITE_TOOLS {
        permissions.remove(&format!("{SERVER}_{tool}"));
    }
}

fn is_write_rule(value: &Value) -> bool {
    value.as_str().is_some_and(|value| {
        WRITE_TOOLS.iter().any(|tool| value == format!("mcp__{SERVER}__{tool}"))
    })
}
