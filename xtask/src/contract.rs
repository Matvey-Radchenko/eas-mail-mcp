mod compat;

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context as _, Result};
use serde_json::Value;

const BASELINE: &str = "contracts/v1.0.json";

pub(crate) fn run(root: &Path, accept: bool) -> Result<()> {
    let current = capture(root)?;
    if accept {
        fs::write(root.join(BASELINE), format!("{}\n", serde_json::to_string_pretty(&current)?))?;
        return Ok(());
    }
    let expected: Value = serde_json::from_str(&fs::read_to_string(root.join(BASELINE))?)?;
    if let Some(path) = first_difference(&expected, &current, "") {
        compat::check(&expected, &current)?;
        anyhow::bail!(
            "public contract changed at {path}; review the semantic diff and run cargo xtask contract accept intentionally"
        );
    }
    Ok(())
}

pub(crate) fn compatibility(root: &Path) -> Result<()> {
    let current = capture(root)?;
    let expected: Value = serde_json::from_str(&fs::read_to_string(root.join(BASELINE))?)?;
    compat::check(&expected, &current)
}

fn capture(root: &Path) -> Result<Value> {
    let output = Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--locked",
            "-p",
            "eas-mail-mcp-harness",
            "--bin",
            "contract-dump",
        ])
        .current_dir(root)
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "contract capture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let current: Value =
        serde_json::from_slice(&output.stdout).context("invalid contract capture")?;
    Ok(current)
}

fn first_difference(expected: &Value, actual: &Value, path: &str) -> Option<String> {
    if expected == actual {
        return None;
    }
    if let (Value::Object(left), Value::Object(right)) = (expected, actual) {
        let keys = left.keys().chain(right.keys()).collect::<std::collections::BTreeSet<_>>();
        for key in keys {
            let child = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
            match (left.get(key), right.get(key)) {
                (Some(left), Some(right)) => {
                    if let Some(path) = first_difference(left, right, &child) {
                        return Some(path);
                    }
                }
                _ => return Some(child),
            }
        }
    }
    Some(path.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn differences_report_a_reviewable_semantic_path() {
        assert!(
            first_difference(&serde_json::json!({"x":1}), &serde_json::json!({"x":1}), "")
                .is_none()
        );
        assert_eq!(
            first_difference(
                &serde_json::json!({"mcp":{"send":{"required":[]}}}),
                &serde_json::json!({"mcp":{"send":{"required":["body"]}}}),
                ""
            ),
            Some("/mcp/send/required".into())
        );
    }
}
