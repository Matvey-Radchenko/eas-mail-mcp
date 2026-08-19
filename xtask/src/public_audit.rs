mod history;

use std::fs;
use std::io::{self, Write as _};
use std::path::Path;

use anyhow::{Context as _, Result};

use crate::command::output;

pub(crate) fn run(root: &Path, denylist: Option<&Path>) -> Result<()> {
    let candidates =
        output(root, "git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard"])?;
    let paths = candidates
        .split('\0')
        .filter(|value| !value.is_empty() && root.join(value).is_file())
        .collect::<Vec<_>>();
    let private_prefix = [".", "private", "/"].concat();
    anyhow::ensure!(
        paths.iter().all(|path| !path.starts_with(&private_prefix)),
        "a file from the private build directory is tracked"
    );
    let patterns = patterns(root, denylist)?;
    let mut findings = Vec::new();
    for relative in &paths {
        let path = root.join(relative);
        let bytes = fs::read(&path).with_context(|| format!("cannot read {relative}"))?;
        scan(relative, &bytes, &patterns, &mut findings);
    }
    let history =
        output(root, "git", ["log", "--all", "--format=fuller", "--patch", "--no-ext-diff"])?;
    scan("Git history text", history.as_bytes(), &patterns, &mut findings);
    history::scan_blobs(root, &patterns, &mut findings)?;
    for finding in &findings {
        writeln!(io::stderr().lock(), "private material: {finding}")?;
    }
    anyhow::ensure!(findings.is_empty(), "public audit failed");
    writeln!(io::stdout().lock(), "public audit passed: {} tracked files", paths.len())?;
    Ok(())
}

fn patterns(root: &Path, denylist: Option<&Path>) -> Result<Vec<String>> {
    let mut patterns = vec![
        ["/", "Users", "/"].concat().to_ascii_lowercase(),
        ["LicenseRef", "-Proprietary"].concat().to_ascii_lowercase(),
    ];
    if let Some(path) = denylist {
        let path = if path.is_absolute() { path.to_owned() } else { root.join(path) };
        let input = fs::read_to_string(&path)
            .with_context(|| format!("cannot read private denylist {}", path.display()))?;
        patterns.extend(
            input
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_ascii_lowercase),
        );
    }
    patterns.sort();
    patterns.dedup();
    Ok(patterns)
}

pub(super) fn scan(label: &str, bytes: &[u8], patterns: &[String], findings: &mut Vec<String>) {
    let lower = String::from_utf8_lossy(bytes).to_ascii_lowercase();
    for pattern in patterns {
        if lower.contains(pattern) {
            findings.push(format!("{label} contains a denied term"));
            break;
        }
    }
}
