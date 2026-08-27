use std::fs::{self, OpenOptions};
use std::os::windows::fs::OpenOptionsExt as _;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use super::{output_with_timeout, path_text, script};
use crate::ErrorCode;

const CHILD_DIRECTORY: &str = "EAS_MAIL_MCP_TEST_CHILD_DIRECTORY";
const CHILD_TEST: &str = "cli::clients::tests::windows_process::descendant_fixture";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn black_box_windows_timeout_stops_batch_descendants_before_rollback() -> anyhow::Result<()> {
    let fixture = BatchFixture::new(false)?;
    let started = Instant::now();
    let error = output_with_timeout(path_text(&fixture.script)?, &[], COMMAND_TIMEOUT)
        .err()
        .ok_or_else(|| anyhow::anyhow!("the waiting descendant must time out"))?;
    assert_eq!(error.envelope.code, ErrorCode::ConfigInvalid);
    assert_eq!(error.envelope.message, "AI client command timed out");
    assert!(started.elapsed() < COMMAND_TIMEOUT + Duration::from_secs(2));
    fixture.assert_rollback_is_safe()?;
    Ok(())
}

#[test]
fn black_box_windows_launcher_exit_does_not_leave_a_pipe_holding_descendant() -> anyhow::Result<()>
{
    let fixture = BatchFixture::new(true)?;
    let started = Instant::now();
    let output = output_with_timeout(path_text(&fixture.script)?, &[], COMMAND_TIMEOUT)?;
    assert!(output.status.success());
    assert!(started.elapsed() < COMMAND_TIMEOUT);
    fixture.assert_rollback_is_safe()?;
    Ok(())
}

#[test]
fn black_box_windows_client_drains_both_output_pipes() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let client = script(
        directory.path(),
        "verbose",
        "for /L %%i in (1,1,10000) do (echo stdout-line& echo stderr-line 1>&2)\r\nexit /b 0",
    )?;
    let output = output_with_timeout(path_text(&client)?, &[], COMMAND_TIMEOUT)?;
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout)?.lines().count(), 10000);
    assert_eq!(String::from_utf8(output.stderr)?.lines().count(), 10000);
    Ok(())
}

// Invoked in a separate test executable by the .cmd fixture, never by production code.
#[test]
fn descendant_fixture() -> anyhow::Result<()> {
    let Some(directory) = std::env::var_os(CHILD_DIRECTORY).map(PathBuf::from) else {
        return Ok(());
    };
    let lease = OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(0)
        .open(directory.join("lease"))?;
    fs::write(directory.join("ready"), [])?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(15) {
        if directory.join("stop").exists() {
            break;
        }
        if directory.join("mutate").exists() {
            fs::write(directory.join("config"), "changed after rollback")?;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    drop(lease);
    Ok(())
}

struct BatchFixture {
    directory: tempfile::TempDir,
    script: PathBuf,
}

impl BatchFixture {
    fn new(background: bool) -> anyhow::Result<Self> {
        let directory = tempfile::tempdir()?;
        let executable = std::env::current_exe()?;
        let invocation =
            format!("\"{}\" --exact {CHILD_TEST} --nocapture", path_text(&executable)?);
        let body = if background {
            format!(
                "start \"\" /b {invocation}\r\n\
                 :wait_for_child\r\n\
                 if not exist \"{}\" goto wait_for_child\r\n\
                 exit /b 0",
                directory.path().join("ready").display(),
            )
        } else {
            invocation
        };
        let script = script(
            directory.path(),
            "client launcher",
            &format!("set \"{CHILD_DIRECTORY}={}\"\r\n{body}", directory.path().display()),
        )?;
        Ok(Self { directory, script })
    }

    fn assert_rollback_is_safe(&self) -> anyhow::Result<()> {
        assert!(self.directory.path().join("ready").exists(), "descendant did not start");
        fs::write(self.directory.path().join("config"), "restored")?;
        fs::write(self.directory.path().join("mutate"), [])?;
        assert!(self.wait_for_lease_release(), "descendant still holds its exclusive file");
        assert_eq!(fs::read_to_string(self.directory.path().join("config"))?, "restored");
        Ok(())
    }

    fn wait_for_lease_release(&self) -> bool {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(1) {
            if fs::File::open(self.directory.path().join("lease")).is_ok() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }
}

impl Drop for BatchFixture {
    fn drop(&mut self) {
        // Let an orphan exit even when a regression causes an assertion to fail.
        drop(fs::write(self.directory.path().join("stop"), []));
        if self.directory.path().join("ready").exists() {
            self.wait_for_lease_release();
        }
    }
}
