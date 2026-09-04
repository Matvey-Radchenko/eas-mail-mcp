use std::path::Path;

use anyhow::Result;

use crate::command::run;

pub(crate) fn check(
    root: &Path,
    hours: u64,
    application: Option<&Path>,
    report: Option<&Path>,
) -> Result<()> {
    anyhow::ensure!(hours >= 8, "release soak requires at least 8 hours");
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
    run(root, executable, args)
}
