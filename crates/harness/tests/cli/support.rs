use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt as _;

use serde_json::Value;

pub(super) fn event_ref(state: &Path, query: &str) -> anyhow::Result<String> {
    let value = json_success(
        state,
        &["calendar".into(), "search".into(), query.into(), "--limit".into(), "1".into()],
    )?;
    string_at(&value, "/data/items/0/event_ref")
}

pub(super) fn write_success(state: &Path, arguments: &[String]) -> anyhow::Result<String> {
    let output = run(state, arguments)?;
    anyhow::ensure!(output.status.success(), "{}", text(&output.stderr)?);
    let value = parse(&output.stdout)?;
    anyhow::ensure!(value.pointer("/data/status").and_then(Value::as_str) == Some("succeeded"));
    let preview = text(&output.stderr)?;
    anyhow::ensure!(preview.contains("Operation:"));
    Ok(preview)
}

pub(super) fn json_success(state: &Path, arguments: &[String]) -> anyhow::Result<Value> {
    let output = run(state, arguments)?;
    anyhow::ensure!(output.status.success(), "{}", text(&output.stderr)?);
    parse(&output.stdout)
}

pub(super) fn human_success(state: &Path, arguments: &[String]) -> anyhow::Result<()> {
    let output = run(state, arguments)?;
    anyhow::ensure!(output.status.success(), "{}", text(&output.stderr)?);
    anyhow::ensure!(!output.stdout.is_empty());
    anyhow::ensure!(serde_json::from_slice::<Value>(&output.stdout).is_err());
    Ok(())
}

pub(super) fn run(state: &Path, arguments: &[String]) -> anyhow::Result<Output> {
    Ok(harness_command(state, arguments).output()?)
}

pub(super) fn run_stdin(state: &Path, arguments: &[String], stdin: &str) -> anyhow::Result<Output> {
    let mut child = harness_command(state, arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("CLI stdin is unavailable"))?
        .write_all(stdin.as_bytes())?;
    Ok(child.wait_with_output()?)
}

fn harness_command(state: &Path, arguments: &[String]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_harness-cli"));
    command.args(arguments).env("EAS_MAIL_HARNESS_STATE_DIR", state);
    #[cfg(windows)]
    command.creation_flags(0x0000_0008);
    command
}

pub(super) fn parse(bytes: &[u8]) -> anyhow::Result<Value> {
    Ok(serde_json::from_slice(bytes)?)
}

pub(super) fn string_at(value: &Value, pointer: &str) -> anyhow::Result<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("missing string at {pointer}"))
}

pub(super) fn text(bytes: &[u8]) -> anyhow::Result<String> {
    Ok(String::from_utf8(bytes.to_vec())?)
}

pub(super) fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

pub(super) fn uuid(value: u8) -> String {
    format!("00000000-0000-4000-8000-{value:012}")
}
