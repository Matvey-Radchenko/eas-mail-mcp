#![expect(
    clippy::indexing_slicing,
    reason = "fixed test fixtures use direct indexing for readable assertions"
)]

use super::*;
use crate::{JournalRecord, OperationJournal as _, OperationStatus, SqliteJournal};

#[test]
fn redacted_failure_exposes_no_internal_error_text() {
    let value = redacted_failure(
        "work".into(),
        AppError::new(ErrorCode::NetworkUnreachable, "private server detail")
            .retryable()
            .remediation("connect VPN"),
    );
    assert_eq!(value["account_id"], "work");
    assert_eq!(value["code"], "NETWORK_UNREACHABLE");
    assert_eq!(value["retryable"], true);
    assert!(!value.to_string().contains("private server detail"));
}

#[test]
fn every_error_code_has_a_stable_uppercase_name() {
    let cases = [
        (ErrorCode::AuthRequired, "AUTH_REQUIRED"),
        (ErrorCode::NetworkUnreachable, "NETWORK_UNREACHABLE"),
        (ErrorCode::ConfigInvalid, "CONFIG_INVALID"),
        (ErrorCode::PolicyBlocked, "POLICY_BLOCKED"),
        (ErrorCode::NotFound, "NOT_FOUND"),
        (ErrorCode::ReferenceExpired, "REFERENCE_EXPIRED"),
        (ErrorCode::ValidationFailed, "VALIDATION_FAILED"),
        (ErrorCode::ProtocolError, "PROTOCOL_ERROR"),
        (ErrorCode::SyncStale, "SYNC_STALE"),
        (ErrorCode::OutcomeUnknown, "OUTCOME_UNKNOWN"),
        (ErrorCode::RemoteWipe, "REMOTE_WIPE"),
        (ErrorCode::IdempotencyConflict, "IDEMPOTENCY_CONFLICT"),
        (ErrorCode::StorageError, "STORAGE_ERROR"),
    ];
    for (value, expected) in cases {
        assert_eq!(code(value), expected);
    }
}

#[test]
fn doctor_remote_wipe_purges_persistent_account_data() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = Paths {
        support: directory.path().join("support"),
        attachments: directory.path().join("attachments"),
        config: directory.path().join("support/config.toml"),
        profiles: directory.path().join("support/profiles.toml"),
        journal: directory.path().join("support/operations.sqlite"),
    };
    paths.ensure()?;
    let account_cache = paths.attachments.join("work");
    std::fs::create_dir_all(&account_cache)?;
    std::fs::write(account_cache.join("fixture.txt"), b"fixture")?;
    let journal = SqliteJournal::open(&paths.journal)?;
    let record = JournalRecord {
        operation_id: "11111111-2222-4333-8444-555555555555".into(),
        account_id: "work".into(),
        kind: "mail_send".into(),
        payload_hmac: "fixture".into(),
        client_id: "11111111-2222-4333-8444-555555555555".into(),
        status: OperationStatus::Pending,
    };
    let _ = journal.begin(&record)?;

    let value = redacted_account_failure(
        &paths,
        "work".into(),
        AppError::new(ErrorCode::RemoteWipe, "fixture wipe"),
    );
    assert_eq!(value["code"], "REMOTE_WIPE");
    assert!(!account_cache.exists());
    assert!(journal.begin(&record)?.inserted);
    Ok(())
}
