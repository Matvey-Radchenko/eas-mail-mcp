use super::*;
use crate::{JournalRecord, OperationJournal as _, OperationStatus};
use clap::Parser as _;

const OPERATION: &str = "11111111-2222-4333-8444-555555555555";

#[test]
fn recovery_commands_ignore_broken_config_and_absent_profiles() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    paths.ensure()?;
    std::fs::write(&paths.config, "invalid configuration fixture")?;
    let journal = seeded(&paths)?;
    journal.finish(OPERATION, OperationStatus::Unknown, 0)?;
    assert_eq!(
        run(&paths, OperationCommand::Get { operation_id: OPERATION.into() })?,
        super::super::CliExit::Success
    );
    assert_eq!(
        run(
            &paths,
            OperationCommand::List(ListArgs {
                account: Some("work".into()),
                status: Some("unknown".into()),
                limit: 20,
            })
        )?,
        super::super::CliExit::Success
    );
    assert!(!paths.profiles.exists());
    assert_eq!(std::fs::read_to_string(&paths.config)?, "invalid configuration fixture");
    assert_eq!(
        journal.lookup(OPERATION)?.map(|record| record.status),
        Some(OperationStatus::Unknown)
    );
    Ok(())
}

#[test]
fn active_pending_operation_survives_inspection_then_recovers_when_owner_exits()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    let journal = seeded(&paths)?;
    let locks = WriteLocks::new(directory.path().join("write-locks"))?;
    let guard =
        locks.try_acquire("work")?.ok_or_else(|| anyhow::anyhow!("fixture lock is busy"))?;
    run(&paths, OperationCommand::Get { operation_id: OPERATION.into() })?;
    assert_eq!(
        journal.lookup(OPERATION)?.map(|record| record.status),
        Some(OperationStatus::Pending)
    );
    drop(guard);
    run(&paths, OperationCommand::Get { operation_id: OPERATION.into() })?;
    assert_eq!(
        journal.lookup(OPERATION)?.map(|record| record.status),
        Some(OperationStatus::Unknown)
    );
    Ok(())
}

#[test]
fn invalid_operation_and_unbounded_filters_are_rejected() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    for (command, expected) in [
        (OperationCommand::Get { operation_id: "not-a-uuid".into() }, ErrorCode::ValidationFailed),
        (OperationCommand::Get { operation_id: OPERATION.into() }, ErrorCode::NotFound),
        (
            OperationCommand::List(ListArgs { account: None, status: None, limit: 0 }),
            ErrorCode::ValidationFailed,
        ),
        (
            OperationCommand::List(ListArgs { account: None, status: None, limit: 101 }),
            ErrorCode::ValidationFailed,
        ),
        (
            OperationCommand::List(ListArgs {
                account: None,
                status: Some("invalid".into()),
                limit: 20,
            }),
            ErrorCode::ValidationFailed,
        ),
        (
            OperationCommand::List(ListArgs {
                account: Some("../other".into()),
                status: None,
                limit: 20,
            }),
            ErrorCode::ValidationFailed,
        ),
    ] {
        assert_eq!(run(&paths, command).map_err(|error| error.envelope.code), Err(expected));
    }
    Ok(())
}

#[test]
fn operation_parser_accepts_documented_commands_and_rejects_unknown_status() {
    for status in ["pending", "succeeded", "failed", "partial", "unknown"] {
        assert!(
            super::super::Cli::try_parse_from([
                "eas-mail-mcp",
                "operation",
                "list",
                "--account",
                "work",
                "--status",
                status,
                "--limit",
                "100",
            ])
            .is_ok()
        );
    }
    assert!(
        super::super::Cli::try_parse_from(["eas-mail-mcp", "operation", "get", OPERATION]).is_ok()
    );
    assert!(
        super::super::Cli::try_parse_from([
            "eas-mail-mcp",
            "operation",
            "list",
            "--status",
            "invalid"
        ])
        .is_err()
    );
}

fn paths(root: &std::path::Path) -> Paths {
    Paths {
        support: root.join("support"),
        attachments: root.join("attachments"),
        config: root.join("support/config.toml"),
        profiles: root.join("support/profiles.toml"),
        journal: root.join("support/operations.sqlite"),
    }
}

fn seeded(paths: &Paths) -> anyhow::Result<SqliteJournal> {
    paths.ensure()?;
    let journal = SqliteJournal::open(&paths.journal)?;
    let _ = journal.begin(&JournalRecord {
        operation_id: OPERATION.into(),
        account_id: "work".into(),
        kind: "mail_send".into(),
        payload_hmac: "synthetic-hmac".into(),
        client_id: OPERATION.into(),
        status: OperationStatus::Pending,
        completed_steps: 0,
    })?;
    Ok(journal)
}
