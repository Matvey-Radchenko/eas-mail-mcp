use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use anyhow::{Context as _, Result};

use super::scan;
use crate::command::output;

pub(super) fn scan_blobs(
    root: &Path,
    patterns: &[String],
    findings: &mut Vec<String>,
) -> Result<()> {
    let commits = output(root, "git", ["--no-replace-objects", "rev-list", "--all"])?;
    let mut blobs = BTreeMap::new();
    for commit in commits.lines().filter(|value| !value.is_empty()) {
        let tree =
            output(root, "git", ["--no-replace-objects", "ls-tree", "-r", "--full-tree", commit])?;
        for entry in tree.lines() {
            if let Some((sha, path)) = parse_blob(entry) {
                blobs.entry(sha.to_owned()).or_insert_with(|| path.to_owned());
            }
        }
    }
    for (sha, path) in blobs {
        let bytes = read_blob(root, &sha)?;
        scan(&format!("Git history blob {path}"), &bytes, patterns, findings);
    }
    Ok(())
}

fn parse_blob(entry: &str) -> Option<(&str, &str)> {
    let (metadata, path) = entry.split_once('\t')?;
    let mut fields = metadata.split_whitespace();
    let _mode = fields.next()?;
    if fields.next()? != "blob" {
        return None;
    }
    let sha = fields.next()?;
    fields.next().is_none().then_some((sha, path))
}

fn read_blob(root: &Path, sha: &str) -> Result<Vec<u8>> {
    let result = Command::new("git")
        .args(["--no-replace-objects", "cat-file", "blob", sha])
        .current_dir(root)
        .output()
        .with_context(|| format!("cannot read Git history blob {sha}"))?;
    anyhow::ensure!(result.status.success(), "cannot read Git history blob {sha}");
    Ok(result.stdout)
}

#[cfg(test)]
mod tests {
    use super::parse_blob;

    #[test]
    fn parses_only_blob_entries() {
        assert_eq!(parse_blob("100644 blob abc123\tREADME.md"), Some(("abc123", "README.md")));
        assert_eq!(parse_blob("040000 tree abc123\tdocs"), None);
        assert_eq!(parse_blob("malformed"), None);
    }
}
