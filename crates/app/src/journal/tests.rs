use super::*;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn status_parser_is_strict_and_complete() -> anyhow::Result<()> {
    for (value, expected) in [
        ("pending", OperationStatus::Pending),
        ("succeeded", OperationStatus::Succeeded),
        ("failed", OperationStatus::Failed),
        ("unknown", OperationStatus::Unknown),
    ] {
        assert_eq!(OperationStatus::parse(value)?, expected);
        assert_eq!(expected.as_str(), value);
    }
    assert!(OperationStatus::parse("invalid").is_err());
    Ok(())
}

#[test]
fn fingerprints_are_deterministic_and_content_sensitive() -> anyhow::Result<()> {
    let key = [7_u8; 32];
    let first = payload_fingerprint(&key, b"first")?;
    assert_eq!(first, payload_fingerprint(&key, b"first")?);
    assert_ne!(first, payload_fingerprint(&key, b"second")?);
    assert_eq!(first.len(), 64);
    Ok(())
}

#[test]
fn pending_is_not_a_terminal_finish_state() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let journal = SqliteJournal::open(&directory.path().join("operations.sqlite"))?;
    let result = journal.finish("missing", OperationStatus::Pending);
    assert!(result.is_err_and(|error| error.envelope.code == ErrorCode::StorageError));
    Ok(())
}

#[cfg(unix)]
#[test]
fn journal_file_is_private() -> anyhow::Result<()> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir()?;
    let path = directory.path().join("operations.sqlite");
    let _journal = SqliteJournal::open(&path)?;
    assert_eq!(fs::metadata(path)?.permissions().mode() & 0o777, 0o600);
    Ok(())
}

#[test]
fn storage_write_lock_serializes_independent_connections() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("operations.sqlite");
    let first_path = path.clone();
    let second_path = path;
    let (acquired_tx, acquired_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first = std::thread::spawn(move || {
        with_storage_write_lock(&first_path, || {
            acquired_tx.send(()).map_err(|_| storage_error())?;
            release_rx.recv().map_err(|_| storage_error())?;
            Ok(())
        })
    });
    acquired_rx.recv_timeout(Duration::from_secs(1))?;

    let (second_tx, second_rx) = mpsc::channel();
    let second = std::thread::spawn(move || {
        with_storage_write_lock(&second_path, || {
            second_tx.send(()).map_err(|_| storage_error())?;
            Ok(())
        })
    });
    assert!(second_rx.recv_timeout(Duration::from_millis(50)).is_err());
    release_tx.send(())?;
    first.join().map_err(|_| anyhow::anyhow!("first lock thread failed"))??;
    second_rx.recv_timeout(Duration::from_secs(1))?;
    second.join().map_err(|_| anyhow::anyhow!("second lock thread failed"))??;
    Ok(())
}
