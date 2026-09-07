use std::path::Path;

use serde_json::{Value, json};

use super::cli_support::{json_success, parse, run, string_at, strings, text, uuid};

fn references(state: &Path) -> anyhow::Result<(String, String)> {
    let listed = json_success(state, &strings(&["mail", "list", "--limit", "2"]))?;
    Ok((
        string_at(&listed, "/data/items/0/mail_ref")?,
        string_at(&listed, "/data/items/1/mail_ref")?,
    ))
}

#[test]
fn mail_property_commands_require_confirmation_and_replay_across_processes() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let (reference, _) = references(state.path())?;
    let commands = [
        strings(&["mail", "move", &reference, "archive"]),
        strings(&["mail", "delete", &reference]),
        strings(&["mail", "set-flag", &reference, "active"]),
        strings(&["mail", "set-flag", &reference, "complete"]),
        strings(&["mail", "set-flag", &reference, "none"]),
        strings(&[
            "mail",
            "set-categories",
            &reference,
            "--category",
            "Project",
            "--category",
            "Review",
        ]),
        strings(&["mail", "set-categories", &reference, "--clear"]),
    ];
    for (index, mut args) in commands.into_iter().enumerate() {
        args.extend(["--idempotency-key".into(), uuid(u8::try_from(index)? + 100)]);
        let preview = run(state.path(), &args)?;
        assert_eq!(preview.status.code(), Some(2));
        assert!(text(&preview.stderr)?.contains("Operation:"));
        args.push("--yes".into());
        let first = json_success(state.path(), &args)?;
        assert_eq!(first.pointer("/data/status").and_then(Value::as_str), Some("succeeded"));
        let replay = run(state.path(), &args)?;
        assert!(replay.status.success());
        assert!(
            !text(&replay.stderr)?.contains("Operation:"),
            "Replay must not prepare another write"
        );
        assert_eq!(
            first.pointer("/data/mail_ref"),
            parse(&replay.stdout)?.pointer("/data/mail_ref")
        );
    }
    Ok(())
}

#[test]
fn property_json_inputs_and_validation_match_flag_inputs() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let (reference, _) = references(state.path())?;
    let cases = [
        ("move", json!({"destination_folder_id":"archive"})),
        ("delete", json!({})),
        ("set-flag", json!({"flag":"active"})),
        ("set-categories", json!({"categories":["Category"]})),
    ];
    let file = state.path().join("input.json");
    for (index, (command, mut input)) in cases.into_iter().enumerate() {
        let object = input.as_object_mut().ok_or_else(|| anyhow::anyhow!("input fixture"))?;
        object.insert("mail_ref".into(), json!(reference));
        object.insert("idempotency_key".into(), json!(uuid(u8::try_from(index)? + 120)));
        std::fs::write(&file, serde_json::to_vec(&input)?)?;
        let args = strings(&["mail", command, "--input", &file.to_string_lossy(), "--yes"]);
        assert_eq!(
            json_success(state.path(), &args)?.pointer("/data/status").and_then(Value::as_str),
            Some("succeeded")
        );
        let mut conflicting = args;
        conflicting.push(reference.clone());
        assert_eq!(run(state.path(), &conflicting)?.status.code(), Some(2));
    }
    for arguments in [
        strings(&["mail", "move", &reference, "missing-folder", "--yes"]),
        strings(&["mail", "set-categories", &reference, "--yes"]),
        strings(&["mail", "set-categories", &reference, "--clear", "--category", "x", "--yes"]),
        strings(&["mail", "set-flag", &reference, "invalid", "--yes"]),
    ] {
        assert!(!run(state.path(), &arguments)?.status.success());
    }
    Ok(())
}

#[test]
fn batch_cli_confirms_all_entries_and_replays_saved_locators() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let (first, second) = references(state.path())?;
    let file = state.path().join("batch.json");
    let input = json!({"items":[
        {"mail_ref":first,"idempotency_key":uuid(140),"action":"move","destination_folder_id":"archive"},
        {"mail_ref":second,"idempotency_key":uuid(141),"action":"set_categories","categories":[]}
    ]});
    std::fs::write(&file, serde_json::to_vec(&input)?)?;
    let mut args = strings(&["mail", "batch", "--input", &file.to_string_lossy()]);
    let declined = run(state.path(), &args)?;
    assert_eq!(declined.status.code(), Some(2));
    assert!(text(&declined.stderr)?.contains("mail_batch"));
    args.push("--yes".into());
    for _ in 0..2 {
        let response = json_success(state.path(), &args)?;
        let items = response
            .pointer("/data/items")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("missing batch"))?;
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(
            |item| item.pointer("/result/status").and_then(Value::as_str) == Some("succeeded")
        ));
    }
    args.insert(0, "--human".into());
    let human = run(state.path(), &args)?;
    assert!(human.status.success());
    assert!(text(&human.stdout)?.contains("succeeded"));
    Ok(())
}

#[test]
fn bulk_read_cli_preserves_limits_and_input_modes() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let (first, second) = references(state.path())?;
    let args = strings(&[
        "mail",
        "get-many",
        &first,
        &second,
        "--body-limit",
        "10",
        "--total-body-limit",
        "12",
    ]);
    let result = json_success(state.path(), &args)?;
    let items = result
        .pointer("/data/items")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("missing bulk"))?;
    assert_eq!(items.len(), 2);
    let count = items
        .iter()
        .filter_map(|item| item.pointer("/mail/body").and_then(Value::as_str))
        .map(|body| body.chars().count())
        .sum::<usize>();
    assert!(count <= 12);
    let file = state.path().join("read.json");
    std::fs::write(
        &file,
        serde_json::to_vec(
            &json!({"mail_refs":[first,second],"body_limit":10,"total_body_limit":12}),
        )?,
    )?;
    let mut json_args = strings(&["mail", "get-many", "--input", &file.to_string_lossy()]);
    assert_eq!(result, json_success(state.path(), &json_args)?);
    json_args.extend(strings(&["--body-limit", "1"]));
    assert_eq!(run(state.path(), &json_args)?.status.code(), Some(2));
    for invalid in [strings(&["mail", "get-many"]), strings(&["mail", "get-many", &first, &first])]
    {
        assert!(!run(state.path(), &invalid)?.status.success());
    }
    Ok(())
}

#[test]
fn explicit_folder_sync_works_with_flags_json_batch_and_uuid_replay() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let (first, second) = references(state.path())?;
    let flag_input = state.path().join("flag.json");
    std::fs::write(
        &flag_input,
        serde_json::to_vec(&json!({
            "mail_ref":first,"flag":"active","idempotency_key":uuid(160)
        }))?,
    )?;
    let commands = [
        strings(&["mail", "mark-read", &first, "read", "--idempotency-key", &uuid(161)]),
        strings(&["mail", "set-flag", "--input", &flag_input.to_string_lossy()]),
        strings(&["mail", "set-categories", &first, "--clear", "--idempotency-key", &uuid(162)]),
    ];
    for mut command in commands {
        command.extend(strings(&["--sync-folder", "--yes"]));
        let first_result = json_success(state.path(), &command)?;
        assert_eq!(first_result.pointer("/data/status").and_then(Value::as_str), Some("succeeded"));
        let replay = run(state.path(), &command)?;
        assert!(replay.status.success());
        assert!(!text(&replay.stderr)?.contains("Operation:"));
        assert_eq!(
            first_result.pointer("/data/operation_id"),
            parse(&replay.stdout)?.pointer("/data/operation_id")
        );
    }
    let file = state.path().join("batch-sync.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({"items":[
            {"mail_ref":first,"action":"set_flag","flag":"none","idempotency_key":uuid(163)},
            {"mail_ref":second,"action":"mark_read","is_read":true,"idempotency_key":uuid(164)}
        ]}))?,
    )?;
    let args =
        strings(&["mail", "batch", "--input", &file.to_string_lossy(), "--sync-folder", "--yes"]);
    for _ in 0..2 {
        let result = json_success(state.path(), &args)?;
        let entries = result
            .pointer("/data/items")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("batch response"))?;
        assert!(
            entries.iter().all(|entry| entry.pointer("/result/status").and_then(Value::as_str)
                == Some("succeeded"))
        );
    }
    Ok(())
}
