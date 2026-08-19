#![expect(
    clippy::indexing_slicing,
    reason = "fixed test fixtures use direct indexing for readable assertions"
)]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};
use toml_edit::DocumentMut;

use super::*;

#[test]
fn executable_version_is_best_effort_diagnostics() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let success = script(directory.path(), "success", "echo 'tool 0.148.0-alpha.9'\nexit 0")?;
    let failure = script(directory.path(), "failure", "echo 'broken' >&2\nexit 7")?;
    assert_eq!(detect_version(path_text(&success)?).as_deref(), Some("tool 0.148.0-alpha.9"));
    assert!(detect_version(path_text(&failure)?).is_none());
    assert!(detect_version("/missing/client").is_none());
    assert!(command(path_text(&success)?, &["ignored"], false)?);
    assert!(!command(path_text(&failure)?, &["ignored"], true)?);
    assert!(command(path_text(&failure)?, &["ignored"], false).is_err());
    assert!(command("/missing/client", &[], false).is_err());
    Ok(())
}

#[test]
fn black_box_client_command_has_a_hard_timeout() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let hanging = script(directory.path(), "hanging", "while :; do :; done")?;
    let started = Instant::now();
    assert!(output_with_timeout(path_text(&hanging)?, &[], Duration::from_millis(50)).is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[test]
fn codex_cleanup_preserves_document_and_removes_generated_write_overrides() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    fs::write(
        &path,
        concat!(
            "# keep this comment\n",
            "[mcp_servers.eas-mail]\n",
            "command = \"/tmp/server\"\n",
            "[mcp_servers.eas-mail.tools.mail_send]\n",
            "approval_mode = \"approve\"\n",
            "keep = true\n",
            "[mcp_servers.eas-mail.tools.mail_reply]\n",
            "approval_mode = \"prompt\"\n",
            "[mcp_servers.eas-mail.tools.custom]\n",
            "approval_mode = \"prompt\"\n",
        ),
    )?;
    remove_codex_generated_approvals(&path)?;
    let content = fs::read_to_string(&path)?;
    assert!(content.contains("# keep this comment"));
    let document = content.parse::<DocumentMut>()?;
    let tools = document["mcp_servers"][SERVER]["tools"]
        .as_table()
        .ok_or_else(|| anyhow::anyhow!("tools table is missing"))?;
    assert_eq!(tools["mail_send"]["keep"].as_bool(), Some(true));
    assert!(tools["mail_send"].get("approval_mode").is_none());
    assert!(tools.get("mail_reply").is_none());
    assert_eq!(tools["custom"]["approval_mode"].as_str(), Some("prompt"));
    assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    fs::write(&path, "not = [toml")?;
    assert!(remove_codex_generated_approvals(&path).is_err());
    Ok(())
}

#[test]
fn codex_cleanup_is_a_noop_for_missing_and_unmanaged_settings() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let missing = directory.path().join("missing.toml");
    remove_codex_generated_approvals(&missing)?;
    assert!(!missing.exists());

    for (index, content) in [
        "theme = \"dark\"\n",
        "[mcp_servers.other]\ncommand = \"other\"\n",
        "[mcp_servers.eas-mail]\ncommand = \"server\"\n",
        concat!("[mcp_servers.eas-mail.tools.mail_send]\n", "approval_mode = \"custom\"\n",),
    ]
    .into_iter()
    .enumerate()
    {
        let path = directory.path().join(format!("unmanaged-{index}.toml"));
        fs::write(&path, content)?;
        remove_codex_generated_approvals(&path)?;
        assert_eq!(fs::read_to_string(path)?, content);
    }
    Ok(())
}

#[test]
fn json_and_jsonc_round_trip_validate_shape() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nested/config.json");
    assert_eq!(read_json(&path, false)?, serde_json::json!({}));
    let value = serde_json::json!({"existing": true});
    write_json(&path, &value)?;
    assert_eq!(read_json(&path, false)?, value);
    assert!(fs::read_to_string(&path)?.ends_with('\n'));

    fs::write(&path, "{ // comment\n existing: true,\n}")?;
    assert_eq!(read_json(&path, true)?["existing"], true);
    assert!(read_json(&path, false).is_err());
    fs::write(&path, "[]")?;
    assert!(read_json(&path, false).is_err());
    Ok(())
}

#[test]
fn config_shape_helpers_reject_existing_wrong_types() -> anyhow::Result<()> {
    let mut document = serde_json::json!({});
    let object = object_entry(&mut document, "permissions")?;
    let ask = array_entry(object, "ask")?;
    ask.push(Value::String("rule".into()));
    assert_eq!(document["permissions"]["ask"][0], "rule");

    let mut wrong_root = Value::Array(Vec::new());
    assert!(object_entry(&mut wrong_root, "x").is_err());
    let mut wrong_child = serde_json::json!({"x": []});
    assert!(object_entry(&mut wrong_child, "x").is_err());
    let mut map = Map::from_iter([("ask".into(), Value::Object(Map::new()))]);
    assert!(array_entry(&mut map, "ask").is_err());
    Ok(())
}

#[test]
fn backups_are_private_and_restore_exact_or_absent_state() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = test_paths(directory.path());
    let source = directory.path().join("client.json");
    assert_eq!(backup(&paths, &source, "missing")?, None);
    fs::write(&source, "before")?;
    let saved = backup(&paths, &source, "client")?
        .ok_or_else(|| anyhow::anyhow!("backup was not created"))?;
    fs::write(&source, "after")?;
    restore(&source, Some(&saved))?;
    assert_eq!(fs::read_to_string(&source)?, "before");
    assert_eq!(fs::metadata(&saved)?.permissions().mode() & 0o777, 0o600);
    assert_eq!(
        fs::metadata(saved.parent().ok_or_else(|| anyhow::anyhow!("backup parent missing"))?)?
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    restore(&source, None)?;
    assert!(!source.exists());
    assert_eq!(paths_to_strings([saved].into_iter()).len(), 1);
    Ok(())
}

#[test]
fn black_box_replace_server_does_not_launch_the_existing_server() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let calls = directory.path().join("calls.log");
    let executable = script(
        directory.path(),
        "client",
        &format!(
            "printf '%s\\n' \"$*\" >> '{}'\n\
             if [ \"$1\" = mcp ] && [ \"$2\" = remove ]; then exit 9; fi\n\
             exit 0",
            calls.display()
        ),
    )?;
    replace_cli_server(path_text(&executable)?, &["mcp", "remove"], &["mcp", "add"])?;
    assert_eq!(fs::read_to_string(calls)?, "mcp remove\nmcp add\n");
    assert_eq!(client_name(ClientKind::Codex), "codex");
    assert_eq!(client_name(ClientKind::Claude), "claude");
    assert_eq!(client_name(ClientKind::Opencode), "opencode");
    Ok(())
}

fn script(directory: &Path, name: &str, body: &str) -> anyhow::Result<PathBuf> {
    let path = directory.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n"))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    Ok(path)
}

fn test_paths(root: &Path) -> Paths {
    Paths {
        support: root.join("support"),
        attachments: root.join("attachments"),
        config: root.join("config.toml"),
        profiles: root.join("profiles.toml"),
        journal: root.join("operations.sqlite"),
    }
}

mod configuration;
