use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use super::{BINARY, digest, make_executable, target_arch, write_manifest};
use crate::command::output;

const AGENT_GUIDE: &str = "docs/agent-installation.ru.md";

pub(super) fn create(root: &Path, dist: &Path, bundles: &[(&str, PathBuf)]) -> Result<PathBuf> {
    anyhow::ensure!(bundles.len() == 2, "handoff requires exactly two architecture bundles");
    let name = format!("{BINARY}-{}-macos-handoff", env!("CARGO_PKG_VERSION"));
    let handoff = dist.join(name);
    fs::create_dir_all(handoff.join("bin/arm64"))?;
    fs::create_dir_all(handoff.join("bin/x86_64"))?;

    let mut binary_paths = Vec::with_capacity(2);
    for (target, bundle) in bundles {
        let architecture = target_arch(target);
        let relative = PathBuf::from("bin").join(architecture).join(BINARY);
        let source = bundle.join("bin").join(BINARY);
        let destination = handoff.join(&relative);
        fs::copy(&source, &destination)
            .with_context(|| format!("cannot copy {}", source.display()))?;
        make_executable(&destination)?;
        binary_paths.push(relative);
    }

    fs::copy(root.join("scripts/install.sh"), handoff.join("install.sh"))?;
    fs::copy(root.join("scripts/uninstall.sh"), handoff.join("uninstall.sh"))?;
    fs::copy(root.join("docs/installation.ru.md"), handoff.join("installation.ru.md"))?;
    fs::copy(root.join(AGENT_GUIDE), handoff.join("INSTALL-FOR-AI-AGENT.md"))?;
    fs::write(handoff.join("TARGET_ARCHS"), "arm64\nx86_64\n")?;
    write_metadata(root, &handoff, &binary_paths)?;
    make_executable(&handoff.join("install.sh"))?;
    make_executable(&handoff.join("uninstall.sh"))?;

    let mut files = vec![
        PathBuf::from("BUILD-METADATA.json"),
        PathBuf::from("INSTALL-FOR-AI-AGENT.md"),
        PathBuf::from("TARGET_ARCHS"),
        PathBuf::from("install.sh"),
        PathBuf::from("installation.ru.md"),
        PathBuf::from("uninstall.sh"),
    ];
    files.extend(binary_paths);
    write_manifest(&handoff, files)?;
    Ok(handoff)
}

fn write_metadata(root: &Path, handoff: &Path, binary_paths: &[PathBuf]) -> Result<()> {
    let source_sha = output(root, "git", ["rev-parse", "HEAD"])?;
    let mut artifacts = serde_json::Map::new();
    for relative in binary_paths {
        let architecture = relative
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow::anyhow!("handoff binary path has no architecture"))?;
        artifacts.insert(
            architecture.to_owned(),
            serde_json::json!({
                "path": relative,
                "sha256": digest(&handoff.join(relative))?,
            }),
        );
    }
    let document = serde_json::json!({
        "format": "macos-dual-architecture-handoff",
        "package_version": env!("CARGO_PKG_VERSION"),
        "source_sha": source_sha.trim(),
        "artifacts": artifacts,
    });
    fs::write(
        handoff.join("BUILD-METADATA.json"),
        format!("{}\n", serde_json::to_string_pretty(&document)?),
    )?;
    Ok(())
}
