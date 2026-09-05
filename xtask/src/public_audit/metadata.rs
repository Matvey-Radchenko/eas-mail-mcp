mod allowlist;

use std::path::Path;
use std::process::Command;

use anyhow::{Context as _, Result};
use sha2::{Digest as _, Sha256};

use super::scan;
use crate::command::output;

pub(super) fn scan_history(
    root: &Path,
    patterns: &[String],
    findings: &mut Vec<String>,
) -> Result<()> {
    let mut allowed = allowlist::Allowlist::load(root)?;
    let commits = output(root, "git", ["--no-replace-objects", "rev-list", "--all"])?;
    for commit in commits.lines().filter(|value| !value.is_empty()) {
        let result = Command::new("git")
            .args(["--no-replace-objects", "cat-file", "commit", commit])
            .current_dir(root)
            .output()
            .context("cannot read Git commit object")?;
        anyhow::ensure!(result.status.success(), "cannot read Git commit object {commit}");
        scan_commit(commit, &result.stdout, &mut allowed, patterns, findings)?;
    }
    allowed.finish()?;
    let patches = output(
        root,
        "git",
        [
            "--no-replace-objects",
            "log",
            "--all",
            "--format=",
            "--patch",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
        ],
    )?;
    scan("Git history patches", patches.as_bytes(), patterns, findings);
    Ok(())
}

fn scan_commit(
    commit: &str,
    bytes: &[u8],
    allowed: &mut allowlist::Allowlist,
    patterns: &[String],
    findings: &mut Vec<String>,
) -> Result<()> {
    let parts = split_commit(bytes)?;
    // This scan is unconditional: messages and non-identity headers cannot use an exception.
    scan(&format!("Git history commit {commit} contents"), &parts.contents, patterns, findings);
    if !allowed.permits(commit, &metadata_hash(&parts.metadata))? {
        scan(
            &format!("Git history commit {commit} author/committer metadata"),
            &parts.metadata,
            patterns,
            findings,
        );
    }
    Ok(())
}

struct CommitParts {
    metadata: Vec<u8>,
    contents: Vec<u8>,
}

fn split_commit(bytes: &[u8]) -> Result<CommitParts> {
    let boundary = bytes
        .windows(2)
        .position(|value| value == b"\n\n")
        .context("Git commit has no header/message boundary")?;
    let (headers, message) = bytes.split_at(boundary);
    let mut author = None;
    let mut committer = None;
    let mut contents = Vec::new();
    let mut identity_header = false;
    for line in headers.split(|byte| *byte == b'\n') {
        anyhow::ensure!(
            !(identity_header && line.starts_with(b" ")),
            "Git identity header has an unsupported continuation"
        );
        identity_header = false;
        if line.starts_with(b"author ") {
            anyhow::ensure!(
                author.replace(line).is_none(),
                "Git commit has duplicate author metadata"
            );
            identity_header = true;
        } else if line.starts_with(b"committer ") {
            anyhow::ensure!(
                committer.replace(line).is_none(),
                "Git commit has duplicate committer metadata"
            );
            identity_header = true;
        } else {
            contents.extend_from_slice(line);
            contents.push(b'\n');
        }
    }
    let author = author.context("Git commit has no author metadata")?;
    let committer = committer.context("Git commit has no committer metadata")?;
    let mut metadata = Vec::new();
    for line in [author, committer] {
        metadata.extend_from_slice(line);
        metadata.push(b'\n');
    }
    contents.extend_from_slice(message);
    Ok(CommitParts { metadata, contents })
}

fn metadata_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod git_tests;
#[cfg(test)]
mod tests;
