use eas_mail_mcp::{AutoReplySetInput, OperationJournal as _, OperationStatus, SqliteJournal};
use serde_json::Value;

use super::cli_support::{
    human_success, json_success, run, run_stdin, strings, text, uuid, write_success,
};

#[test]
fn auto_reply_cli_get_set_preview_and_replay() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let get = strings(&["mail", "auto-reply", "get", "--account", "example"]);
    let value = json_success(state.path(), &get)?;
    assert_eq!(value.pointer("/data/state").and_then(Value::as_str), Some("disabled"));
    human_success(
        state.path(),
        &strings(&["--human", "mail", "auto-reply", "get", "--account", "example"]),
    )?;
    let key = uuid(81);
    let args = strings(&[
        "mail",
        "auto-reply",
        "set",
        "--account",
        "example",
        "--state",
        "enabled",
        "--internal-message",
        "Away fixture",
        "--idempotency-key",
        &key,
        "--yes",
    ]);
    let preview = write_success(state.path(), &args)?;
    assert!(preview.contains("Away fixture") && preview.contains("External audience"));
    let replay = run(state.path(), &args)?;
    assert!(replay.status.success());
    assert!(!text(&replay.stderr)?.contains("Operation:"));
    let json =
        serde_json::json!({"account_id":"example", "state":"disabled", "idempotency_key":uuid(82)});
    let result = run_stdin(
        state.path(),
        &strings(&["mail", "auto-reply", "set", "--input", "-", "--yes"]),
        &json.to_string(),
    )?;
    assert!(result.status.success(), "{}", text(&result.stderr)?);
    Ok(())
}

#[test]
fn partial_auto_reply_cli_replay_preserves_machine_warning_without_confirmation()
-> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let input: AutoReplySetInput = serde_json::from_value(serde_json::json!({
        "account_id":"example", "state":"enabled", "internal_message":"Away fixture",
        "idempotency_key":uuid(83)
    }))?;
    let seed = run_stdin(
        state.path(),
        &strings(&["mail", "auto-reply", "set", "--input", "-", "--yes"]),
        &serde_json::to_string(&input)?,
    )?;
    assert!(seed.status.success());
    let journal = SqliteJournal::open(&state.path().join("journal"))?;
    // Simulate an acknowledged Set whose read-back could not be verified before restart.
    journal.finish(&input.idempotency_key, OperationStatus::Partial, 1)?;
    let output = run_stdin(
        state.path(),
        &strings(&["mail", "auto-reply", "set", "--input", "-"]),
        &serde_json::to_string(&input)?,
    )?;
    assert_eq!(output.status.code(), Some(3));
    assert!(!text(&output.stderr)?.contains("Operation:"));
    let response: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(response.pointer("/data/status").and_then(Value::as_str), Some("partial"));
    assert_eq!(response.pointer("/warnings/0/code").and_then(Value::as_str), Some("PARTIAL_WRITE"));
    assert_eq!(response.pointer("/warnings/0/account_id").and_then(Value::as_str), Some("example"));
    assert_eq!(
        response.pointer("/warnings/0/operation_id").and_then(Value::as_str),
        Some(input.idempotency_key.as_str())
    );
    Ok(())
}
