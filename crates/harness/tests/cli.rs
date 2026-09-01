use std::path::Path;

use serde_json::Value;

#[path = "cli/support.rs"]
mod cli_support;
#[path = "cli/series.rs"]
mod series;

use self::cli_support::{
    event_ref, human_success, json_success, parse, run, run_stdin, string_at, strings, text, uuid,
    write_success,
};

#[test]
fn read_commands_are_bounded_and_references_cross_processes() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    json_success(state.path(), &strings(&["account", "list"]))?;
    json_success(state.path(), &strings(&["folder", "list"]))?;
    human_success(state.path(), &strings(&["--human", "account", "list"]))?;
    human_success(state.path(), &strings(&["--human", "folder", "list"]))?;

    let default_page = json_success(state.path(), &strings(&["mail", "list"]))?;
    assert_eq!(
        default_page.pointer("/data/items").and_then(Value::as_array).map(Vec::len),
        Some(50)
    );
    assert_eq!(
        default_page.pointer("/data/results_truncated").and_then(Value::as_bool),
        Some(true)
    );

    let listed = json_success(state.path(), &strings(&["mail", "list", "--limit", "2"]))?;
    assert_eq!(listed.pointer("/data/items").and_then(Value::as_array).map(Vec::len), Some(2));
    assert_eq!(listed.pointer("/data/results_truncated").and_then(Value::as_bool), Some(true));
    assert!(listed.pointer("/data/next_cursor").is_none());
    let mail_ref = string_at(&listed, "/data/items/0/mail_ref")?;

    let searched =
        json_success(state.path(), &strings(&["mail", "search", "report", "--limit", "1"]))?;
    assert_eq!(searched.pointer("/data/items").and_then(Value::as_array).map(Vec::len), Some(1));
    json_success(state.path(), &["mail".into(), "get".into(), mail_ref.clone()])?;
    human_success(
        state.path(),
        &["--human".into(), "mail".into(), "get".into(), mail_ref.clone()],
    )?;
    let attachments =
        json_success(state.path(), &["mail".into(), "attachments".into(), mail_ref.clone()])?;
    human_success(
        state.path(),
        &["--human".into(), "mail".into(), "attachments".into(), mail_ref],
    )?;
    let attachment_ref = string_at(&attachments, "/data/attachments/0/attachment_ref")?;
    let download =
        json_success(state.path(), &["mail".into(), "download".into(), attachment_ref.clone()])?;
    let downloaded = string_at(&download, "/data/path")?;
    assert!(Path::new(&downloaded).is_file());
    std::fs::remove_file(downloaded)?;
    human_success(
        state.path(),
        &["--human".into(), "mail".into(), "download".into(), attachment_ref],
    )?;

    let all = json_success(state.path(), &strings(&["mail", "list", "--all"]))?;
    assert_eq!(all.pointer("/data/items").and_then(Value::as_array).map(Vec::len), Some(250));
    assert_eq!(all.pointer("/data/results_truncated").and_then(Value::as_bool), Some(false));
    let search_all = json_success(state.path(), &strings(&["mail", "search", "report", "--all"]))?;
    assert_eq!(
        search_all.pointer("/data/items").and_then(Value::as_array).map(Vec::len),
        Some(250)
    );

    let human = run(state.path(), &strings(&["--human", "mail", "list", "--limit", "1"]))?;
    assert!(human.status.success());
    assert!(text(&human.stdout)?.contains("Quarterly update"));
    assert!(serde_json::from_slice::<Value>(&human.stdout).is_err());
    Ok(())
}

#[test]
fn calendar_read_commands_use_compact_runtime_contracts() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let common = [
        "--participant",
        "person@example.invalid",
        "--from",
        "2023-11-14",
        "--to",
        "2023-11-14",
        "--time-zone",
        "UTC",
        "--working-hours",
        "mon,tue,wed,thu,fri@09:00-18:00",
    ];
    let mut availability = strings(&["calendar", "availability"]);
    availability.extend(strings(&common));
    json_success(state.path(), &availability)?;
    availability.insert(0, "--human".into());
    human_success(state.path(), &availability)?;

    let mut slots = strings(&["calendar", "find-slots"]);
    slots.extend(strings(&common));
    slots.extend(strings(&["--duration", "30"]));
    json_success(state.path(), &slots)?;
    slots.insert(0, "--human".into());
    human_success(state.path(), &slots)?;

    let search =
        json_success(state.path(), &strings(&["calendar", "search", "planning", "--limit", "1"]))?;
    let event_ref = string_at(&search, "/data/items/0/event_ref")?;
    json_success(state.path(), &["calendar".into(), "get".into(), event_ref.clone()])?;
    human_success(state.path(), &["--human".into(), "calendar".into(), "get".into(), event_ref])?;
    let agenda = json_success(
        state.path(),
        &strings(&[
            "calendar",
            "agenda",
            "--from",
            "2023-11-14",
            "--to",
            "2023-11-16",
            "--time-zone",
            "UTC",
        ]),
    )?;
    assert!(agenda.pointer("/data/items").and_then(Value::as_array).is_some());
    human_success(
        state.path(),
        &strings(&[
            "--human",
            "calendar",
            "agenda",
            "--from",
            "2023-11-14",
            "--to",
            "2023-11-16",
            "--time-zone",
            "UTC",
        ]),
    )?;
    Ok(())
}

#[test]
fn mail_writes_preview_confirm_replay_and_exit_codes() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let listed = json_success(state.path(), &strings(&["mail", "list", "--limit", "1"]))?;
    let mail_ref = string_at(&listed, "/data/items/0/mail_ref")?;

    mail_target_writes(state.path(), mail_ref)?;
    send_confirmation_cases(state.path())
}

fn mail_target_writes(state: &Path, mail_ref: String) -> anyhow::Result<()> {
    let writes = [
        (
            vec![
                "mail".into(),
                "mark-read".into(),
                mail_ref.clone(),
                "read".into(),
                "--idempotency-key".into(),
                uuid(1),
                "--yes".into(),
            ],
            ["Sender: \"Sender <sender@example.invalid>\"", "New read state: \"true\""].as_slice(),
        ),
        (
            vec![
                "mail".into(),
                "reply".into(),
                mail_ref.clone(),
                "--body".into(),
                "reply body".into(),
                "--idempotency-key".into(),
                uuid(2),
                "--yes".into(),
            ],
            [
                "To: \"sender@example.invalid\"",
                "Subject: \"Re: Quarterly update\"",
                "Body: \"reply body\"",
            ]
            .as_slice(),
        ),
        (
            vec![
                "mail".into(),
                "forward".into(),
                mail_ref,
                "--to".into(),
                "forward@example.invalid".into(),
                "--body".into(),
                "forward body".into(),
                "--idempotency-key".into(),
                uuid(3),
                "--yes".into(),
            ],
            [
                "To: \"forward@example.invalid\"",
                "Subject: \"Fwd: Quarterly update\"",
                "Body: \"forward body\"",
            ]
            .as_slice(),
        ),
    ];
    for (arguments, preview_fields) in writes {
        let output = run(state, &arguments)?;
        assert!(output.status.success(), "{}", text(&output.stderr)?);
        let preview = text(&output.stderr)?;
        assert!(preview.contains("Operation:"));
        for field in preview_fields {
            assert!(preview.contains(field), "missing preview field {field}: {preview}");
        }
        parse(&output.stdout)?;
    }
    Ok(())
}

fn send_confirmation_cases(state: &Path) -> anyhow::Result<()> {
    let send = strings(&[
        "mail",
        "send",
        "--account",
        "example",
        "--to",
        "recipient@example.invalid",
        "--subject",
        "Safe subject",
        "--body",
        "line one\n\u{1b}[31mline two",
        "--idempotency-key",
        "44444444-4444-4444-8444-444444444444",
    ]);
    let rejected = run(state, &send)?;
    assert_eq!(rejected.status.code(), Some(2));
    let rejected_preview = text(&rejected.stderr)?;
    assert!(rejected_preview.contains("\\n\\u001b[31m"));

    let mut accepted = send.clone();
    accepted.push("--yes".into());
    let accepted = run(state, &accepted)?;
    assert!(accepted.status.success());
    assert!(text(&accepted.stdout)?.contains("Exchange confirmed"));

    let replay = run(state, &send)?;
    assert!(replay.status.success());
    assert!(!text(&replay.stderr)?.contains("Operation:"));
    assert!(text(&replay.stdout)?.contains("prior operation"));
    let mut human_replay = send.clone();
    human_replay.insert(0, "--human".into());
    human_success(state, &human_replay)?;

    let conflict = strings(&[
        "mail",
        "send",
        "--account",
        "example",
        "--to",
        "recipient@example.invalid",
        "--subject",
        "Changed payload",
        "--body",
        "line one",
        "--idempotency-key",
        "44444444-4444-4444-8444-444444444444",
        "--yes",
    ]);
    let conflict = run(state, &conflict)?;
    assert_eq!(conflict.status.code(), Some(1));
    assert!(text(&conflict.stderr)?.contains("IDEMPOTENCY_CONFLICT"));
    assert!(!text(&conflict.stderr)?.contains("Operation:"));
    Ok(())
}

#[test]
fn calendar_writes_cover_all_lifecycle_commands() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let organizer = event_ref(state.path(), "planning")?;
    let personal = event_ref(state.path(), "personal")?;
    let received = event_ref(state.path(), "received")?;

    let create = strings(&[
        "calendar",
        "create",
        "--account",
        "example",
        "--subject",
        "CLI event",
        "--start",
        "2023-11-20T10:00:00Z",
        "--end",
        "2023-11-20T11:00:00Z",
        "--time-zone",
        "UTC",
        "--body",
        "event body",
        "--idempotency-key",
        "55555555-5555-4555-8555-555555555555",
        "--yes",
    ]);
    let preview = write_success(state.path(), &create)?;
    assert!(preview.contains("Subject: \"CLI event\""));
    assert!(preview.contains("Body: \"event body\""));
    let preview = write_success(
        state.path(),
        &[
            "calendar".into(),
            "update".into(),
            organizer.clone(),
            "--subject".into(),
            "Updated event".into(),
            "--idempotency-key".into(),
            uuid(6),
            "--yes".into(),
        ],
    )?;
    assert!(preview.contains("Subject: \"Updated event\""));
    assert!(preview.contains("Removed attendees: \"\""));
    let preview = write_success(
        state.path(),
        &[
            "calendar".into(),
            "delete".into(),
            personal,
            "--idempotency-key".into(),
            uuid(7),
            "--yes".into(),
        ],
    )?;
    assert!(preview.contains("Operation: \"calendar_delete\""));
    assert!(preview.contains("Subject: \"Planning\""));
    let preview = write_success(
        state.path(),
        &[
            "calendar".into(),
            "cancel".into(),
            organizer,
            "--comment".into(),
            "Cancelled".into(),
            "--idempotency-key".into(),
            uuid(8),
            "--yes".into(),
        ],
    )?;
    assert!(preview.contains("Operation: \"calendar_cancel\""));
    assert!(preview.contains("Comment: \"Cancelled\""));
    let preview = write_success(
        state.path(),
        &[
            "calendar".into(),
            "respond".into(),
            received,
            "accept".into(),
            "--comment".into(),
            "Accepted".into(),
            "--idempotency-key".into(),
            uuid(9),
            "--yes".into(),
        ],
    )?;
    assert!(preview.contains("Response: \"accept\""));
    assert!(preview.contains("Comment: \"Accepted\""));
    Ok(())
}

#[test]
fn json_input_stdin_and_usage_fail_closed() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let listed = json_success(state.path(), &strings(&["mail", "list", "--limit", "1"]))?;
    let mail_ref = string_at(&listed, "/data/items/0/mail_ref")?;
    let input = state.path().join("get.json");
    std::fs::write(&input, serde_json::to_vec(&serde_json::json!({ "mail_ref": mail_ref }))?)?;
    json_success(
        state.path(),
        &["mail".into(), "get".into(), "--input".into(), input.display().to_string()],
    )?;
    let stdin_json = serde_json::to_string(&serde_json::json!({
        "mail_ref": string_at(&listed, "/data/items/0/mail_ref")?
    }))?;
    let stdin_result =
        run_stdin(state.path(), &strings(&["mail", "get", "--input", "-"]), &stdin_json)?;
    assert!(stdin_result.status.success());
    parse(&stdin_result.stdout)?;

    let invalid = state.path().join("invalid.json");
    std::fs::write(
        &invalid,
        serde_json::to_vec(&serde_json::json!({ "mail_ref": "ref", "unknown": true }))?,
    )?;
    let failed = run(
        state.path(),
        &["mail".into(), "get".into(), "--input".into(), invalid.display().to_string()],
    )?;
    assert_eq!(failed.status.code(), Some(2));
    assert!(failed.stdout.is_empty());

    let body_stdin = strings(&[
        "mail",
        "send",
        "--account",
        "example",
        "--to",
        "recipient@example.invalid",
        "--subject",
        "stdin body",
        "--body-stdin",
        "--yes",
    ]);
    let output = run_stdin(state.path(), &body_stdin, "body from stdin\n")?;
    assert!(output.status.success());
    assert!(text(&output.stderr)?.contains("body from stdin\\n"));
    let output = parse(&output.stdout)?;
    let operation_id = string_at(&output, "/data/operation_id")?;
    uuid::Uuid::parse_str(&operation_id)?;

    let send_input = state.path().join("send.json");
    std::fs::write(
        &send_input,
        serde_json::to_vec(&serde_json::json!({
            "account_id": "example",
            "to": ["recipient@example.invalid"],
            "cc": [],
            "bcc": [],
            "subject": "JSON input",
            "body": "body"
        }))?,
    )?;
    let output = json_success(
        state.path(),
        &[
            "mail".into(),
            "send".into(),
            "--input".into(),
            send_input.display().to_string(),
            "--yes".into(),
        ],
    )?;
    uuid::Uuid::parse_str(&string_at(&output, "/data/operation_id")?)?;
    Ok(())
}
