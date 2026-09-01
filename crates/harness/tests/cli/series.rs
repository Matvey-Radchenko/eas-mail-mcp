use super::{human_success, json_success, parse, run, run_stdin, string_at, strings, text};
use anyhow::Context as _;
use serde_json::{Value, json};

#[test]
fn directory_cli_flags_json_and_human_output_are_bounded() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let args = strings(&["people", "search", "--query", "Test", "--limit", "1"]);
    let result = json_success(state.path(), &args)?;
    assert_eq!(result.pointer("/data/items").and_then(Value::as_array).map(Vec::len), Some(1));
    assert_eq!(result.pointer("/data/results_truncated"), Some(&Value::Bool(true)));
    let mut human = args;
    human.insert(0, "--human".into());
    human_success(state.path(), &human)?;
    let result = run_stdin(
        state.path(),
        &strings(&["people", "search", "--input", "-"]),
        r#"{"query":"Test","account_id":"example"}"#,
    )?;
    assert!(result.status.success(), "{}", text(&result.stderr)?);
    parse(&result.stdout)?;
    for payload in
        [r#"{"query":" "}"#, r#"{"query":"Test","limit":51}"#, r#"{"query":"Test","unknown":true}"#]
    {
        let result =
            run_stdin(state.path(), &strings(&["people", "search", "--input", "-"]), payload)?;
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
    }
    Ok(())
}

#[test]
fn calendar_recurrence_flags_and_json_use_the_same_preview() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let args = strings(&[
        "calendar",
        "create",
        "--account",
        "example",
        "--subject",
        "Series CLI",
        "--start",
        "2026-08-24T10:00:00Z",
        "--end",
        "2026-08-24T11:00:00Z",
        "--time-zone",
        "UTC",
        "--repeat",
        "weekly",
        "--repeat-weekday",
        "mon",
        "--repeat-weekday",
        "wed",
        "--repeat-count",
        "5",
        "--yes",
    ]);
    let result = run(state.path(), &args)?;
    assert!(result.status.success(), "{}", text(&result.stderr)?);
    assert!(text(&result.stderr)?.contains("every 1 week(s) on Mon, Wed; 5 occurrences"));
    assert!(string_at(&parse(&result.stdout)?, "/data/event_ref")?.starts_with("ref1.event."));
    let result = run_stdin(state.path(), &strings(&["calendar","create","--input","-","--yes"]), &json!({
        "account_id":"example","subject":"Monthly",
        "schedule":{"kind":"all_day","start_date":"2026-08-24","end_date":"2026-08-25","time_zone":"UTC"},
        "recurrence":{"frequency":"monthly","day_of_month":24,"end":{"mode":"never"}}
    }).to_string())?;
    assert!(result.status.success(), "{}", text(&result.stderr)?);
    for tail in [
        vec!["--repeat", "weekly"],
        vec!["--repeat-count", "3"],
        vec!["--repeat", "weekly", "--repeat-count", "2", "--repeat-forever"],
        vec!["--repeat", "weekly", "--repeat-weekday", "nonsense", "--repeat-count", "2"],
    ] {
        let mut invalid = strings(&[
            "calendar",
            "create",
            "--account",
            "example",
            "--subject",
            "Invalid",
            "--all-day-start",
            "2026-08-24",
            "--all-day-end",
            "2026-08-25",
            "--time-zone",
            "UTC",
            "--yes",
        ]);
        invalid.extend(strings(&tail));
        let output = run(state.path(), &invalid)?;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    Ok(())
}

#[test]
fn occurrence_reference_from_cli_agenda_is_usable_by_another_process() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let agenda = json_success(
        state.path(),
        &strings(&[
            "calendar",
            "agenda",
            "--from",
            "2023-11-15",
            "--to",
            "2023-11-17",
            "--time-zone",
            "UTC",
        ]),
    )?;
    let occurrence = agenda
        .pointer("/data/items")
        .and_then(Value::as_array)
        .context("agenda")?
        .iter()
        .find(|item| item.get("recurring") == Some(&Value::Bool(true)))
        .context("recurrence")?;
    let reference = occurrence.get("event_ref").and_then(Value::as_str).context("ref")?;
    let result = json_success(state.path(), &strings(&["calendar", "get", reference]))?;
    assert_eq!(result.pointer("/data/starts_at"), occurrence.get("starts_at"));
    let absent_scope = run(
        state.path(),
        &strings(&["calendar", "update", reference, "--subject", "Unsafe", "--yes"]),
    )?;
    assert_eq!(absent_scope.status.code(), Some(2));
    let missing_confirmation = run(
        state.path(),
        &strings(&[
            "calendar",
            "update",
            reference,
            "--scope",
            "occurrence",
            "--subject",
            "Not sent",
        ]),
    )?;
    assert_eq!(missing_confirmation.status.code(), Some(2));
    let updated = run(
        state.path(),
        &strings(&[
            "calendar",
            "update",
            reference,
            "--scope",
            "occurrence",
            "--subject",
            "Changed",
            "--yes",
        ]),
    )?;
    assert!(updated.status.success(), "{}", text(&updated.stderr)?);
    assert!(text(&updated.stderr)?.contains("Occurrence"));
    assert!(text(&updated.stderr)?.contains("Original occurrence:"));
    assert_eq!(parse(&updated.stdout)?.pointer("/data/status"), Some(&json!("succeeded")));
    Ok(())
}
