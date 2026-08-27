use std::ffi::OsStr;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context as _, Result};

pub(crate) fn run<I, S>(root: &Path, program: &str, arguments: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let arguments =
        arguments.into_iter().map(|value| value.as_ref().to_owned()).collect::<Vec<_>>();
    writeln!(io::stderr(), "+ {program} {}", display(&arguments))?;
    let status = Command::new(resolve_executable(program))
        .args(&arguments)
        .current_dir(root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("cannot run {program}; install the pinned development tools"))?;
    anyhow::ensure!(status.success(), "command failed: {program} {}", display(&arguments));
    Ok(())
}

pub(crate) fn run_env<I, S>(
    root: &Path,
    program: &str,
    arguments: I,
    environment: &[(&str, &str)],
) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let arguments =
        arguments.into_iter().map(|value| value.as_ref().to_owned()).collect::<Vec<_>>();
    writeln!(io::stderr(), "+ {program} {}", display(&arguments))?;
    let status = Command::new(resolve_executable(program))
        .args(&arguments)
        .envs(environment.iter().copied())
        .current_dir(root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("cannot run {program}; install the pinned development tools"))?;
    anyhow::ensure!(status.success(), "command failed: {program} {}", display(&arguments));
    Ok(())
}

pub(crate) fn output<I, S>(root: &Path, program: &str, arguments: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let result =
        Command::new(resolve_executable(program)).args(arguments).current_dir(root).output()?;
    anyhow::ensure!(result.status.success(), "command failed: {program}");
    String::from_utf8(result.stdout).context("command output is not UTF-8")
}

#[cfg(windows)]
fn resolve_executable(program: &str) -> PathBuf {
    let requested = Path::new(program);
    if requested.extension().is_some() {
        return requested.to_owned();
    }
    let extensions = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if requested.components().count() > 1 {
        return with_first_existing_extension(requested, &extensions)
            .unwrap_or_else(|| requested.to_owned());
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .find_map(|directory| {
            with_first_existing_extension(&directory.join(requested), &extensions)
        })
        .unwrap_or_else(|| requested.to_owned())
}

#[cfg(not(windows))]
fn resolve_executable(program: &str) -> PathBuf {
    PathBuf::from(program)
}

#[cfg(windows)]
fn with_first_existing_extension(path: &Path, extensions: &[String]) -> Option<PathBuf> {
    extensions.iter().find_map(|extension| {
        let mut candidate = path.as_os_str().to_owned();
        candidate.push(extension);
        let candidate = PathBuf::from(candidate);
        candidate.is_file().then_some(candidate)
    })
}

fn display(arguments: &[std::ffi::OsString]) -> String {
    arguments.iter().map(|value| value.to_string_lossy()).collect::<Vec<_>>().join(" ")
}
