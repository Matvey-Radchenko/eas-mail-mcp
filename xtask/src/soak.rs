use std::path::Path;

use anyhow::Result;

use crate::command::run;

pub(crate) fn check(
    root: &Path,
    hours: u64,
    application: Option<&Path>,
    report: Option<&Path>,
    four_hour_exception: bool,
) -> Result<()> {
    validate_duration(hours, four_hour_exception, env!("CARGO_PKG_VERSION"))?;
    if application.is_none() {
        run(root, "cargo", ["build", "--release", "--locked", "--package", "eas-mail-mcp"])?;
    }
    run(
        root,
        "cargo",
        [
            "build",
            "--release",
            "--locked",
            "--package",
            "eas-mail-mcp-harness",
            "--features",
            "soak",
            "--bin",
            "soak-harness",
        ],
    )?;
    let suffix = std::env::consts::EXE_SUFFIX;
    let harness = root.join(format!("target/release/soak-harness{suffix}"));
    let fallback = root.join(format!("target/release/eas-mail-mcp{suffix}"));
    let application = application.unwrap_or(&fallback);
    let executable = harness.to_str().ok_or_else(|| anyhow::anyhow!("soak path is not UTF-8"))?;
    let hours = hours.to_string();
    let mut args =
        vec!["--application".as_ref(), application.as_os_str(), "--hours".as_ref(), hours.as_ref()];
    if let Some(report) = report {
        args.extend(["--report".as_ref(), report.as_os_str()]);
    }
    if four_hour_exception {
        args.push("--four-hour-1-0-exception".as_ref());
    }
    run(root, executable, args)
}

fn validate_duration(hours: u64, exception: bool, version: &str) -> Result<()> {
    if exception {
        anyhow::ensure!(
            version == "1.0.0" && hours == 4,
            "the four-hour exception applies only to a four-hour release 1.0.0 soak"
        );
    } else {
        anyhow::ensure!(hours >= 8, "release soak requires at least 8 hours");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortened_soak_requires_the_exact_one_time_exception() -> Result<()> {
        validate_duration(4, true, "1.0.0")?;
        validate_duration(8, false, "1.1.0")?;
        for (hours, enabled, version) in
            [(4, false, "1.0.0"), (3, true, "1.0.0"), (8, true, "1.0.0"), (4, true, "1.0.1")]
        {
            assert!(validate_duration(hours, enabled, version).is_err());
        }
        Ok(())
    }
}
