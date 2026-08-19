#![expect(
    clippy::indexing_slicing,
    reason = "fixed test fixtures use direct indexing for readable assertions"
)]

use std::fs;

use toml_edit::DocumentMut;

use super::super::*;
use super::{script, test_paths};

#[test]
fn client_files_select_supported_version_one_locations() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let files = ClientFiles::from_home(directory.path());
    assert!(files.codex.ends_with(".codex/config.toml"));
    assert!(files.claude_mcp.ends_with(".claude.json"));
    assert!(files.claude_settings.ends_with(".claude/settings.json"));
    assert!(files.opencode.ends_with("opencode.jsonc"));

    fs::create_dir_all(directory.path().join(".config/opencode"))?;
    fs::write(directory.path().join(".config/opencode/opencode.json"), "{}")?;
    let files = ClientFiles::from_home(directory.path());
    assert!(files.opencode.ends_with("opencode.json"));
    fs::write(directory.path().join(".config/opencode/opencode.jsonc"), "{}")?;
    let files = ClientFiles::from_home(directory.path());
    assert!(files.opencode.ends_with("opencode.jsonc"));
    Ok(())
}

#[test]
fn opencode_configure_and_remove_preserve_unmanaged_entries() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = test_paths(directory.path());
    let config = directory.path().join("opencode.jsonc");
    fs::write(
        &config,
        concat!(
            "{ // retained\n",
            " mcp: { existing: { type: 'remote' } },\n",
            " permission: { shell: 'deny', 'eas-mail_mail_send': 'allow' }\n",
            "}",
        ),
    )?;
    let bridge = directory.path().join("mail-mcp");
    let backups = configure_opencode(&paths, &config, &bridge)?;
    assert_eq!(backups.len(), 1);
    let document = read_json(&config, true)?;
    assert_eq!(document["mcp"]["existing"]["type"], "remote");
    assert_eq!(document["mcp"][SERVER]["command"][0], bridge.to_string_lossy().as_ref());
    assert!(document["permission"].get("eas-mail_mail_send").is_none());

    assert_eq!(unconfigure_opencode(&paths, &config)?.len(), 1);
    let removed = read_json(&config, true)?;
    assert!(removed["mcp"].get(SERVER).is_none());
    assert_eq!(removed["permission"]["shell"], "deny");
    assert!(unconfigure_opencode(&paths, &directory.path().join("missing"))?.is_empty());
    Ok(())
}

#[test]
fn black_box_claude_configure_and_remove_manage_only_mail_rules() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = test_paths(directory.path());
    let files = ClientFiles::from_home(directory.path());
    let executable = script(directory.path(), "claude", "exit 0")?;
    fs::create_dir_all(files.claude_settings.parent().ok_or_else(path_error)?)?;
    fs::write(
        &files.claude_settings,
        r#"{"permissions":{"ask":["existing-rule","mcp__eas-mail__mail_send"],"allow":["existing-allow","mcp__eas-mail__mail_reply"]}}"#,
    )?;
    let backups = configure_claude(
        &paths,
        &files,
        path_text(&executable)?,
        &directory.path().join("bridge"),
    )?;
    assert_eq!(backups.len(), 1);
    let document = read_json(&files.claude_settings, false)?;
    let ask = document["permissions"]["ask"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("ask rules are missing"))?;
    assert_eq!(ask.as_slice(), &[serde_json::json!("existing-rule")]);
    let allow = document["permissions"]["allow"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("allow rules are missing"))?;
    assert_eq!(allow.as_slice(), &[serde_json::json!("existing-allow")]);

    let removed = unconfigure_claude(&paths, &files, path_text(&executable)?)?;
    assert!(!removed.is_empty());
    let document = read_json(&files.claude_settings, false)?;
    assert_eq!(document["permissions"]["ask"], serde_json::json!(["existing-rule"]));
    assert_eq!(document["permissions"]["allow"], serde_json::json!(["existing-allow"]));
    Ok(())
}

#[test]
fn claude_configuration_failure_restores_both_config_files() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = test_paths(directory.path());
    let files = ClientFiles::from_home(directory.path());
    let executable = script(directory.path(), "claude-failure", "exit 7")?;
    fs::create_dir_all(files.claude_settings.parent().ok_or_else(path_error)?)?;
    fs::write(&files.claude_mcp, "mcp-before")?;
    fs::write(&files.claude_settings, r#"{"permissions":{"ask":["before"]}}"#)?;

    let result =
        configure_claude(&paths, &files, path_text(&executable)?, &directory.path().join("bridge"));
    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&files.claude_mcp)?, "mcp-before");
    assert_eq!(
        fs::read_to_string(&files.claude_settings)?,
        r#"{"permissions":{"ask":["before"]}}"#
    );
    Ok(())
}

#[test]
fn black_box_codex_configure_removes_generated_approval_rules_and_calls_cli() -> anyhow::Result<()>
{
    let directory = tempfile::tempdir()?;
    let paths = test_paths(directory.path());
    let config = directory.path().join("config.toml");
    fs::write(
        &config,
        concat!(
            "[mcp_servers.eas-mail]\n",
            "command = \"old\"\n",
            "[mcp_servers.eas-mail.tools.mail_send]\n",
            "approval_mode = \"approve\"\n",
        ),
    )?;
    let executable = script(directory.path(), "codex", "exit 0")?;
    let backups = configure_codex(
        &paths,
        &config,
        path_text(&executable)?,
        &directory.path().join("bridge"),
    )?;
    assert_eq!(backups.len(), 1);
    let document = fs::read_to_string(&config)?.parse::<DocumentMut>()?;
    assert!(document["mcp_servers"][SERVER].get("tools").is_none());
    assert_eq!(unconfigure_cli(&paths, path_text(&executable)?, config, false)?.len(), 1);
    Ok(())
}

fn path_error() -> std::io::Error {
    std::io::Error::other("path has no parent")
}
