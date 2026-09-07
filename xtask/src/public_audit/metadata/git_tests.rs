use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};

use super::{metadata_hash, scan_history, split_commit};

#[test]
fn approved_identity_does_not_exempt_history_patches_or_blobs() -> Result<()> {
    let repository = Repository::new()?;
    let blob =
        repository.git(&["hash-object", "-w", "--stdin"], b"private-identity in a file\n")?;
    let tree =
        repository.git(&["mktree"], format!("100644 blob {blob}\tfixture.txt\n").as_bytes())?;
    let raw = format!(
        "tree {tree}\nauthor private-identity <fixture@example.invalid> 1 +0000\ncommitter Example <fixture@example.invalid> 2 +0000\n\nPublic message\n"
    );
    let commit =
        repository.git(&["hash-object", "-t", "commit", "-w", "--stdin"], raw.as_bytes())?;
    repository.git(&["update-ref", "refs/heads/main", &commit], b"")?;
    let entries = serde_json::json!({"schema_version":1,"entries":[{
        "commit_sha":commit,"metadata_sha256":metadata_hash(&split_commit(raw.as_bytes())?.metadata)}]});
    fs::write(repository.0.join(".gitignore"), ".private/\n")?;
    fs::create_dir(repository.0.join(".private"))?;
    fs::write(
        repository.0.join(".private/public-audit-history-metadata-allowlist.json"),
        serde_json::to_vec(&entries)?,
    )?;
    let mut findings = Vec::new();
    scan_history(&repository.0, &["private-identity".into()], &mut findings)?;
    assert_eq!(findings.len(), 1);
    assert!(findings.first().is_some_and(|value| value.contains("patches")));
    super::super::history::scan_blobs(&repository.0, &["private-identity".into()], &mut findings)?;
    assert_eq!(findings.len(), 2);
    assert!(findings.iter().any(|value| value.contains("blob")));
    Ok(())
}

struct Repository(PathBuf);

impl Repository {
    fn new() -> Result<Self> {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root =
            std::env::temp_dir().join(format!("eas-audit-test-{}-{unique}", std::process::id()));
        fs::create_dir(&root)?;
        let repository = Self(root);
        repository.git(&["init", "--quiet"], b"")?;
        Ok(repository)
    }

    fn git(&self, arguments: &[&str], input: &[u8]) -> Result<String> {
        let hooks = format!("core.hooksPath={}", self.0.join("absent-hooks").display());
        let mut child = Command::new("git")
            .args(["-c", &hooks])
            .args(arguments)
            .current_dir(&self.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child.stdin.take().context("test Git stdin is unavailable")?.write_all(input)?;
        let output = child.wait_with_output()?;
        anyhow::ensure!(output.status.success(), "temporary test Git command failed");
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
