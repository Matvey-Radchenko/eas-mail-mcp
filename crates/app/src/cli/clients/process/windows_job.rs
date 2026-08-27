use std::io::{self, Read};
use std::process::{Command, ExitStatus, Output};
use std::thread;
use std::time::{Duration, Instant};

use process_wrap::std::{ChildWrapper, CommandWrap, JobObject};

use crate::{AppError, ErrorCode, Result};

pub(super) fn output(command: Command, timeout: Duration) -> Result<Output> {
    // JobObject assigns the suspended process before resuming it, so a batch
    // launcher cannot spawn descendants outside the job during startup.
    let child = CommandWrap::from(command).wrap(JobObject).spawn().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            AppError::new(ErrorCode::NotFound, "AI client executable is unavailable")
        } else {
            command_error("cannot start AI client command in a Windows job")
        }
    })?;

    thread::scope(|scope| {
        let mut child = JobChild { child, terminated: false };
        let stdout = child.child.stdout().take().ok_or_else(output_error)?;
        let stderr = child.child.stderr().take().ok_or_else(output_error)?;
        let stdout = thread::Builder::new()
            .spawn_scoped(scope, move || read_pipe(stdout))
            .map_err(|_| output_error())?;
        let stderr = thread::Builder::new()
            .spawn_scoped(scope, move || read_pipe(stderr))
            .map_err(|_| output_error())?;

        let status = child.wait_timeout(timeout);
        // Also stop descendants when the launcher exits first: they can retain
        // output pipes or write configuration after the caller starts rollback.
        child.terminate().map_err(|_| command_error("cannot stop AI client process tree"))?;
        let status = status
            .map_err(|_| command_error("cannot monitor AI client command"))?
            .ok_or_else(|| command_error("AI client command timed out"))?;
        let stdout = stdout.join().map_err(|_| output_error())?.map_err(|_| output_error())?;
        let stderr = stderr.join().map_err(|_| output_error())?.map_err(|_| output_error())?;
        Ok(Output { status, stdout, stderr })
    })
}

struct JobChild {
    child: Box<dyn ChildWrapper>,
    terminated: bool,
}

impl JobChild {
    fn wait_timeout(&mut self, timeout: Duration) -> io::Result<Option<ExitStatus>> {
        let started = Instant::now();
        loop {
            // Only the launcher determines command completion. The outer job
            // remains owned until all remaining descendants are terminated.
            if let Some(status) = self.child.inner_mut().try_wait()? {
                return Ok(Some(status));
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Ok(None);
            }
            thread::sleep(remaining.min(Duration::from_millis(10)));
        }
    }

    fn terminate(&mut self) -> io::Result<()> {
        self.child.start_kill()?;
        self.child.wait()?;
        self.terminated = true;
        Ok(())
    }
}

impl Drop for JobChild {
    fn drop(&mut self) {
        if !self.terminated && self.child.start_kill().is_ok() {
            drop(self.child.wait());
        }
    }
}

fn read_pipe(mut pipe: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn command_error(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ConfigInvalid, message)
}

fn output_error() -> AppError {
    command_error("cannot read AI client command output")
}
