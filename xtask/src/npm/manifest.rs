use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context as _, Result};
use serde::Deserialize;

const ROOT_PACKAGE: &str = "eas-mail-mcp";
const PLATFORM_PACKAGES: [(&str, &str); 2] =
    [("eas-mail-mcp-darwin-arm64", "arm64"), ("eas-mail-mcp-darwin-x64", "x64")];
const LICENSE_FILES: [&str; 2] = ["LICENSE-MIT", "LICENSE-APACHE"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Package {
    name: String,
    version: String,
    #[serde(default)]
    os: Vec<String>,
    #[serde(default)]
    cpu: Vec<String>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    scripts: BTreeMap<String, String>,
    #[serde(default)]
    optional_dependencies: BTreeMap<String, String>,
    #[serde(default)]
    bin: BTreeMap<String, String>,
}

pub(super) fn verify(root: &Path) -> Result<()> {
    let version = workspace_version(root)?;
    let root_package = read_package(root, ROOT_PACKAGE)?;
    anyhow::ensure!(root_package.name == ROOT_PACKAGE, "root npm package has the wrong name");
    verify_common(&root_package, &version)?;
    anyhow::ensure!(root_package.os == ["darwin"], "root npm package must be macOS-only");
    anyhow::ensure!(root_package.cpu == ["arm64", "x64"], "root npm package has wrong CPUs");
    anyhow::ensure!(
        root_package.bin.get(ROOT_PACKAGE).is_some_and(|path| path == "bin/eas-mail-mcp.js"),
        "root npm package does not expose the launcher"
    );
    verify_files(&root_package, "bin/eas-mail-mcp.js")?;

    for (name, cpu) in PLATFORM_PACKAGES {
        let package = read_package(root, name)?;
        anyhow::ensure!(package.name == name, "{name} manifest has the wrong package name");
        verify_common(&package, &version)?;
        anyhow::ensure!(package.os == ["darwin"], "{name} must be macOS-only");
        anyhow::ensure!(package.cpu == [cpu], "{name} has the wrong CPU selector");
        verify_files(&package, "bin/eas-mail-mcp")?;
        anyhow::ensure!(
            root_package.optional_dependencies.get(name).is_some_and(|value| value == &version),
            "root package must pin {name} to {version}"
        );
    }
    anyhow::ensure!(
        root_package.optional_dependencies.len() == PLATFORM_PACKAGES.len(),
        "root package contains an unsupported platform dependency"
    );
    verify_launcher(root)
}

pub(super) fn workspace_version(root: &Path) -> Result<String> {
    let document = fs::read_to_string(root.join("Cargo.toml"))?;
    let value: toml::Value = toml::from_str(&document)?;
    value
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .context("workspace.package.version is missing")
}

fn read_package(root: &Path, name: &str) -> Result<Package> {
    let path = root.join("npm").join(name).join("package.json");
    let document =
        fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_str(&document).with_context(|| format!("invalid {}", path.display()))
}

fn verify_common(package: &Package, version: &str) -> Result<()> {
    anyhow::ensure!(package.version == version, "{} version must be {version}", package.name);
    anyhow::ensure!(package.scripts.is_empty(), "{} must not use lifecycle scripts", package.name);
    Ok(())
}

fn verify_files(package: &Package, executable: &str) -> Result<()> {
    let mut expected = vec![executable, LICENSE_FILES[0], LICENSE_FILES[1]];
    expected.sort_unstable();
    let mut actual = package.files.iter().map(String::as_str).collect::<Vec<_>>();
    actual.sort_unstable();
    anyhow::ensure!(actual == expected, "{} exposes unexpected files", package.name);
    Ok(())
}

fn verify_launcher(root: &Path) -> Result<()> {
    let path = root.join("npm/eas-mail-mcp/bin/eas-mail-mcp.js");
    let launcher = fs::read_to_string(&path)?;
    anyhow::ensure!(
        launcher.starts_with("#!/usr/bin/env node\n"),
        "npm launcher needs a Node shebang"
    );
    for (name, _) in PLATFORM_PACKAGES {
        anyhow::ensure!(launcher.contains(name), "npm launcher does not select {name}");
    }
    anyhow::ensure!(!launcher.contains("postinstall"), "npm launcher contains lifecycle logic");
    Ok(())
}
