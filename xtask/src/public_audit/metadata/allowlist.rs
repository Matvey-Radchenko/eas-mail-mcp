use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context as _, Result};
use serde::Deserialize;

const LOCAL_FILE: &str = ".private/public-audit-history-metadata-allowlist.json";

#[derive(Default)]
pub(super) struct Allowlist {
    entries: BTreeMap<String, String>,
    seen: BTreeSet<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema_version: u8,
    entries: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    commit_sha: String,
    metadata_sha256: String,
}

impl Allowlist {
    pub(super) fn load(root: &Path) -> Result<Self> {
        let path = root.join(LOCAL_FILE);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(_) => anyhow::bail!("cannot inspect local history metadata allowlist"),
        };
        anyhow::ensure!(
            metadata.is_file() && metadata.len() <= 65_536,
            "history metadata allowlist must be a regular file no larger than 64 KiB"
        );
        let ignored = Command::new("git")
            .args(["check-ignore", "--quiet", "--no-index", "--", LOCAL_FILE])
            .current_dir(root)
            .status()
            .context("cannot check local allowlist exclusion")?;
        anyhow::ensure!(ignored.success(), "history metadata allowlist must be Git-ignored");
        Self::parse(&fs::read(path).context("cannot read local history metadata allowlist")?)
    }

    pub(super) fn parse(bytes: &[u8]) -> Result<Self> {
        let document: Document = serde_json::from_slice(bytes)
            .map_err(|_| anyhow::anyhow!("history metadata allowlist JSON is invalid"))?;
        anyhow::ensure!(
            document.schema_version == 1,
            "unsupported history metadata allowlist schema"
        );
        let mut entries = BTreeMap::new();
        for entry in document.entries {
            anyhow::ensure!(
                matches!(entry.commit_sha.len(), 40 | 64)
                    && hex(&entry.commit_sha)
                    && entry.metadata_sha256.len() == 64
                    && hex(&entry.metadata_sha256),
                "history metadata allowlist requires complete lowercase SHA values"
            );
            anyhow::ensure!(
                entries.insert(entry.commit_sha, entry.metadata_sha256).is_none(),
                "history metadata allowlist contains a duplicate commit"
            );
        }
        Ok(Self { entries, seen: BTreeSet::new() })
    }

    pub(super) fn permits(&mut self, commit: &str, hash: &str) -> Result<bool> {
        let Some(expected) = self.entries.get(commit) else { return Ok(false) };
        anyhow::ensure!(
            expected == hash,
            "history metadata fingerprint differs for commit {commit}"
        );
        self.seen.insert(commit.to_owned());
        Ok(true)
    }

    pub(super) fn finish(self) -> Result<()> {
        anyhow::ensure!(
            self.entries.len() == self.seen.len(),
            "history metadata allowlist contains a commit outside audited history"
        );
        Ok(())
    }
}

fn hex(value: &str) -> bool {
    value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
