use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use wait_timeout::ChildExt as _;

use super::WriteLocks;
use crate::{
    ErrorCode, JournalRecord, OperationJournal as _, OperationStatus, RandomIds, Runtime,
    SqliteJournal, SystemClock,
};

const DIRECTORY_ENV: &str = "EAS_MAIL_MCP_JOURNAL_TEST_DIRECTORY";
const MODE_ENV: &str = "EAS_MAIL_MCP_JOURNAL_TEST_MODE";
const LABEL_ENV: &str = "EAS_MAIL_MCP_JOURNAL_TEST_LABEL";
const CHILD_TEST: &str = "write_lock::process_tests::journal_process_child";
const PROCESS_TIMEOUT: Duration = Duration::from_secs(10);

#[test]
fn crash_after_external_action_recovers_unknown_without_repeating_action() -> anyhow::Result<()> {
    exercise_crash("action", 0)
}

#[test]
fn crashed_process_recovery_preserves_confirmed_checkpoint() -> anyhow::Result<()> {
    exercise_crash("checkpoint", 1)
}

fn exercise_crash(mode: &str, completed_steps: u32) -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let mut writer = Worker::spawn(directory.path(), mode, "writer")?;
    writer.ready()?;
    let journal = Arc::new(SqliteJournal::open(&directory.path().join("journal.sqlite"))?);
    let _reader = runtime(directory.path(), Arc::clone(&journal))?;
    let original = record();
    let before = journal.lookup(&original.operation_id)?;
    assert!(before.is_some_and(|row| {
        row.status == OperationStatus::Pending && row.completed_steps == completed_steps
    }));
    let locks = WriteLocks::new(directory.path().join("write-locks"))?;
    assert!(locks.try_acquire("work")?.is_none());
    assert_eq!(fs::read_to_string(directory.path().join("external-actions"))?, "applied\n");

    writer.kill()?;
    assert!(locks.try_acquire("work")?.is_some());
    let _restarted = runtime(directory.path(), Arc::clone(&journal))?;
    let replay = journal.begin(&original)?;
    assert!(!replay.inserted);
    assert_eq!(replay.record.status, OperationStatus::Unknown);
    assert_eq!(replay.record.completed_steps, completed_steps);
    let mut changed = original.clone();
    changed.payload_hmac = "different-payload".into();
    assert!(
        journal
            .begin(&changed)
            .is_err_and(|error| error.envelope.code == ErrorCode::IdempotencyConflict)
    );

    // A fresh process executes the same operation flow and must not repeat its external action.
    Worker::spawn(directory.path(), "replay", "replay")?.wait()?;
    assert_eq!(fs::read_to_string(directory.path().join("external-actions"))?, "applied\n");
    assert_eq!(
        journal.lookup(&original.operation_id)?.map(|r| r.status),
        Some(OperationStatus::Unknown)
    );
    Ok(())
}

#[test]
fn separate_processes_migrate_one_legacy_database_transactionally() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.sqlite");
    {
        let connection = rusqlite::Connection::open(&path)?;
        connection.execute_batch(
            "CREATE TABLE operations (
            operation_id TEXT PRIMARY KEY, account_id TEXT NOT NULL, kind TEXT NOT NULL,
            payload_hmac TEXT NOT NULL, client_id TEXT NOT NULL, status TEXT NOT NULL,
            created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
            INSERT INTO operations VALUES ('legacy', 'work', 'mail_send', 'fingerprint',
            'client', 'pending', 1234, 1234);",
        )?;
    }
    let mut first = Worker::spawn(directory.path(), "migration", "first")?;
    let mut second = Worker::spawn(directory.path(), "migration", "second")?;
    first.ready()?;
    second.ready()?;
    fs::write(directory.path().join("start-migration"), [])?;
    first.wait()?;
    second.wait()?;
    let connection = rusqlite::Connection::open(&path)?;
    let version: u32 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    assert_eq!(version, 1);
    assert_eq!(
        connection.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))?,
        "ok"
    );
    check_migrated(&SqliteJournal::open(&path)?)
}

// Spawned by the parent tests with a private environment and --exact; normal test runs are no-ops.
#[test]
fn journal_process_child() -> anyhow::Result<()> {
    let Some(directory) = std::env::var_os(DIRECTORY_ENV) else {
        return Ok(());
    };
    let directory = PathBuf::from(directory);
    let mode = std::env::var(MODE_ENV)?;
    let ready = directory.join(format!("{}.ready", std::env::var(LABEL_ENV)?));
    if mode == "migration" {
        fs::write(ready, [])?;
        wait_file(&directory.join("start-migration"))?;
        return check_migrated(&SqliteJournal::open(&directory.join("journal.sqlite"))?);
    }
    run_writer(&directory, &mode, &ready)
}

fn run_writer(directory: &Path, mode: &str, ready: &Path) -> anyhow::Result<()> {
    let journal = Arc::new(SqliteJournal::open(&directory.join("journal.sqlite"))?);
    let _runtime = runtime(directory, Arc::clone(&journal))?;
    let locks = WriteLocks::new(directory.join("write-locks"))?;
    let _guard =
        locks.try_acquire("work")?.ok_or_else(|| anyhow::anyhow!("writer lock unavailable"))?;
    let operation = record();
    let begin = journal.begin(&operation)?;
    if !begin.inserted {
        anyhow::ensure!(mode == "replay" && begin.record.status == OperationStatus::Unknown);
        return Ok(());
    }
    let mut actions =
        OpenOptions::new().create(true).append(true).open(directory.join("external-actions"))?;
    actions.write_all(b"applied\n")?;
    actions.sync_all()?;
    if mode == "replay" {
        // If recovery or deduplication regresses, the parent observes a duplicate durable action.
        journal.finish(&operation.operation_id, OperationStatus::Succeeded, 0)?;
        return Ok(());
    }
    if mode == "checkpoint" {
        journal.checkpoint(&operation.operation_id, 1)?;
    }
    fs::write(ready, [])?;
    std::thread::sleep(PROCESS_TIMEOUT * 3);
    anyhow::bail!("parent did not terminate the paused writer")
}

fn runtime(directory: &Path, journal: Arc<SqliteJournal>) -> anyhow::Result<Runtime> {
    Ok(Runtime::with_dependencies(
        Vec::new(),
        journal,
        Arc::new(SystemClock),
        Arc::new(RandomIds),
        vec![0; 32],
        directory.join("attachments"),
    )?)
}

fn record() -> JournalRecord {
    JournalRecord {
        operation_id: "11111111-2222-4333-8444-555555555555".into(),
        account_id: "work".into(),
        kind: "mail_send".into(),
        payload_hmac: "stable-fingerprint".into(),
        client_id: "client".into(),
        status: OperationStatus::Pending,
        completed_steps: 0,
    }
}

fn check_migrated(journal: &SqliteJournal) -> anyhow::Result<()> {
    let entry = journal.inspect("legacy")?.ok_or_else(|| anyhow::anyhow!("missing legacy row"))?;
    assert_eq!(entry.record.status, OperationStatus::Pending);
    assert_eq!(entry.record.payload_hmac, "fingerprint");
    assert_eq!(entry.record.completed_steps, 0);
    assert_eq!(entry.created_at, 1234);
    assert_eq!(entry.updated_at, 1234);
    assert_eq!(entry.result_locator, None);
    Ok(())
}

struct Worker {
    child: Child,
    ready: PathBuf,
    log: PathBuf,
}

impl Worker {
    fn spawn(directory: &Path, mode: &str, label: &str) -> anyhow::Result<Self> {
        let log = directory.join(format!("{label}.log"));
        let output = fs::File::create(&log)?;
        let child = Command::new(std::env::current_exe()?)
            .args(["--exact", CHILD_TEST, "--nocapture", "--test-threads=1"])
            .env(DIRECTORY_ENV, directory)
            .env(MODE_ENV, mode)
            .env(LABEL_ENV, label)
            .stdin(Stdio::null())
            .stdout(output.try_clone()?)
            .stderr(output)
            .spawn()?;
        Ok(Self { child, ready: directory.join(format!("{label}.ready")), log })
    }

    fn ready(&mut self) -> anyhow::Result<()> {
        let deadline = Instant::now() + PROCESS_TIMEOUT;
        while !self.ready.exists() {
            anyhow::ensure!(
                self.child.try_wait()?.is_none(),
                "child exited: {}",
                fs::read_to_string(&self.log)?
            );
            anyhow::ensure!(
                Instant::now() < deadline,
                "child did not become ready: {}",
                fs::read_to_string(&self.log)?
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    fn kill(&mut self) -> anyhow::Result<()> {
        self.child.kill()?;
        let status = self.child.wait()?;
        anyhow::ensure!(!status.success(), "paused child unexpectedly exited successfully");
        Ok(())
    }

    fn wait(&mut self) -> anyhow::Result<()> {
        let status = self
            .child
            .wait_timeout(PROCESS_TIMEOUT)?
            .ok_or_else(|| anyhow::anyhow!("child timed out: {}", self.log.display()))?;
        anyhow::ensure!(status.success(), "child failed: {}", fs::read_to_string(&self.log)?);
        Ok(())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn wait_file(path: &Path) -> anyhow::Result<()> {
    let deadline = Instant::now() + PROCESS_TIMEOUT;
    while !path.exists() {
        anyhow::ensure!(Instant::now() < deadline, "start signal timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
