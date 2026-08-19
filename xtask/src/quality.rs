use std::fs;
use std::io::{self, Write as _};
use std::path::Path;

use anyhow::Result;

use crate::command::{output, run, run_env};
use crate::{files, goldens, npm, profile, public_audit};

pub(crate) fn check(root: &Path) -> Result<()> {
    verify_toolchain(root)?;
    profile::verify(root, Path::new("profile.example.toml"))?;
    npm::verify(root)?;
    run(root, "cargo", ["fmt", "--all", "--", "--check"])?;
    run(
        root,
        "cargo",
        [
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run_env(
        root,
        "cargo",
        ["doc", "--workspace", "--all-features", "--no-deps", "--locked"],
        &[("RUSTDOCFLAGS", "-D warnings")],
    )?;
    files::check(root)?;
    run(root, "sh", ["-n", "scripts/install.sh", "scripts/uninstall.sh"])?;
    goldens::run(root, false)?;
    test(root)?;
    coverage(root)?;
    run(root, "cargo", ["deny", "check"])?;
    secrets(root)?;
    if root.join(".git").exists() {
        let private_denylist = root.join(".private/public-audit-denylist.txt");
        let denylist = private_denylist.exists().then_some(private_denylist.as_path());
        public_audit::run(root, denylist)?;
    }
    Ok(())
}

pub(crate) fn test(root: &Path) -> Result<()> {
    run(root, "cargo", ["nextest", "run", "--workspace", "--all-features", "--locked"])
}

pub(crate) fn secrets(root: &Path) -> Result<()> {
    scan_plaintext(root)?;
    run(
        root,
        "gitleaks",
        ["detect", "--no-git", "--redact", "--source", ".", "--config", ".gitleaks.toml"],
    )?;
    if root.join(".git").exists() {
        run(root, "gitleaks", ["git", "--redact", "--config", ".gitleaks.toml", "."])?;
    }
    let private = root.join(".private");
    if private.exists() {
        scan_plaintext(&private)?;
        run(
            root,
            "gitleaks",
            [
                "detect",
                "--no-git",
                "--redact",
                "--source",
                ".private",
                "--config",
                ".gitleaks.toml",
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn live(root: &Path, self_write: bool) -> Result<()> {
    let mut arguments = vec![
        "run",
        "--locked",
        "--package",
        "eas-mail-mcp-harness",
        "--features",
        "live",
        "--bin",
        "live-harness",
        "--",
    ];
    if self_write {
        arguments.push("--self-write");
    }
    run(root, "cargo", arguments)
}

pub(crate) fn fuzz(root: &Path, seconds: u64) -> Result<()> {
    let fuzz = root.join("fuzz");
    let time = format!("-max_total_time={seconds}");
    for target in ["wbxml_decode", "sync_parse"] {
        run(&fuzz, "cargo", ["+nightly", "fuzz", "run", target, "--", &time])?;
    }
    Ok(())
}

pub(crate) fn mutants(root: &Path) -> Result<()> {
    run(
        root,
        "cargo",
        [
            "mutants",
            "--package",
            "eas-mail-protocol",
            "--test-package",
            "eas-mail-protocol",
            "--test-package",
            "eas-mail-mcp-harness",
            "--test-tool",
            "nextest",
            "--jobs",
            "4",
            "--minimum-test-timeout",
            "60",
            "--no-shuffle",
        ],
    )
}

fn verify_toolchain(root: &Path) -> Result<()> {
    let rustc = output(root, "rustc", ["--version"])?;
    anyhow::ensure!(
        rustc.starts_with("rustc 1.95.0 "),
        "rustc 1.95.0 is required, found {}",
        rustc.trim()
    );
    Ok(())
}

fn coverage(root: &Path) -> Result<()> {
    fs::create_dir_all(root.join("diagnostics"))?;
    run(
        root,
        "cargo",
        [
            "llvm-cov",
            "nextest",
            "--workspace",
            "--all-features",
            "--locked",
            "--ignore-filename-regex",
            "(crates/harness|xtask|crates/app/src/(main.rs|cli/mod.rs))",
            "--fail-under-lines",
            "85",
            "--fail-under-functions",
            "80",
            "--lcov",
            "--output-path",
            "diagnostics/coverage-workspace.lcov",
        ],
    )?;
    run(
        root,
        "cargo",
        [
            "llvm-cov",
            "nextest",
            "--package",
            "eas-mail-protocol",
            "--all-features",
            "--locked",
            "--fail-under-lines",
            "90",
            "--fail-under-functions",
            "85",
            "--lcov",
            "--output-path",
            "diagnostics/coverage-eas.lcov",
        ],
    )
}

fn scan_plaintext(root: &Path) -> Result<()> {
    let mut findings = Vec::new();
    for path in files::text_files(root)? {
        let relative = path.strip_prefix(root).unwrap_or(&path);
        for (index, line) in fs::read_to_string(&path)?.lines().enumerate() {
            if private_key(line) || suspicious_assignment(line) {
                findings.push(format!("{}:{}", relative.display(), index + 1));
            }
        }
    }
    for finding in &findings {
        writeln!(io::stderr(), "potential secret: {finding}")?;
    }
    anyhow::ensure!(findings.is_empty(), "plaintext secret scan failed");
    Ok(())
}

fn private_key(line: &str) -> bool {
    let begin = "BEGIN";
    let suffix = "PRIVATE KEY";
    ["RSA", "EC", "OPENSSH", ""]
        .iter()
        .map(|kind| {
            if kind.is_empty() {
                format!("{begin} {suffix}")
            } else {
                format!("{begin} {kind} {suffix}")
            }
        })
        .any(|marker| line.contains(&marker))
}

fn suspicious_assignment(line: &str) -> bool {
    let lower = line.trim_start().to_ascii_lowercase();
    let Some(value) = password_value(&lower) else {
        return false;
    };
    !matches!(value, "" | "fixture" | "fixture-value" | "redacted" | "example")
}

fn password_value(line: &str) -> Option<&str> {
    let rest =
        line.strip_prefix("password").or_else(|| line.strip_prefix("\"password\""))?.trim_start();
    let rest = rest.strip_prefix('=').or_else(|| rest.strip_prefix(':'))?.trim_start();
    rest.strip_prefix('"')?.split('"').next()
}

#[cfg(test)]
mod tests {
    use super::{private_key, suspicious_assignment};

    #[test]
    fn plaintext_secret_detection_ignores_code_and_named_fixtures() {
        assert!(!suspicious_assignment("rpassword = \"7.4.0\""));
        assert!(!suspicious_assignment("prompt_password(\"Exchange password: \")"));
        assert!(!suspicious_assignment("password: \"fixture-value\""));
        assert!(!suspicious_assignment("password = \"\""));
        assert!(suspicious_assignment("password = \"actual-secret-value\""));
        assert!(suspicious_assignment("\"password\": \"actual-secret-value\""));
    }

    #[test]
    fn private_key_detection_builds_markers_without_self_matching() {
        let rsa = format!("-----{} {} {}-----", "BEGIN", "RSA", "PRIVATE KEY");
        let generic = format!("-----{} {}-----", "BEGIN", "PRIVATE KEY");
        assert!(private_key(&rsa));
        assert!(private_key(&generic));
        assert!(!private_key("let suffix = PRIVATE KEY"));
    }
}
