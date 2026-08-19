mod manifest;

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use crate::command::{output, run, run_env};
use crate::delivery::{
    BINARY, MAX_BINARY_BYTES, digest, make_executable, remap_flags, sign_macho,
    verify_binary_strings, verify_macho,
};

const DEPLOYMENT_TARGET: &str = "14.0";
const PACKAGES: [(&str, &str); 2] = [
    ("eas-mail-mcp-darwin-arm64", "aarch64-apple-darwin"),
    ("eas-mail-mcp-darwin-x64", "x86_64-apple-darwin"),
];

pub(crate) fn verify(root: &Path) -> Result<()> {
    manifest::verify(root)
}

pub(crate) fn pack(root: &Path) -> Result<()> {
    anyhow::ensure!(cfg!(target_os = "macos"), "npm packaging requires macOS");
    verify(root)?;
    let version = manifest::workspace_version(root)?;
    let dist = root.join("dist/npm");
    let staging = dist.join("staging");
    prepare_directory(&dist)?;
    fs::create_dir_all(&staging)?;

    let rustflags = remap_flags(root)?;
    let mut archives = Vec::with_capacity(PACKAGES.len() + 1);
    for (package, target) in PACKAGES {
        build_target(root, target, &rustflags)?;
        let package_root = stage_platform(root, &staging, package, target)?;
        archives.push(pack_package(&package_root, &dist, package, &version)?);
    }

    let root_package = stage_root(root, &staging)?;
    let root_archive = pack_package(&root_package, &dist, BINARY, &version)?;
    archives.push(root_archive.clone());
    write_checksums(&dist, &archives)?;
    smoke_install(root, &dist, &root_archive, &archives)?;
    Ok(())
}

fn prepare_directory(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).with_context(|| format!("cannot clear {}", path.display()))?;
    }
    fs::create_dir_all(path).with_context(|| format!("cannot create {}", path.display()))
}

fn build_target(root: &Path, target: &str, rustflags: &str) -> Result<()> {
    run_env(
        root,
        "cargo",
        ["build", "--release", "--locked", "--target", target, "--package", BINARY],
        &[("MACOSX_DEPLOYMENT_TARGET", DEPLOYMENT_TARGET), ("RUSTFLAGS", rustflags)],
    )
}

fn stage_platform(root: &Path, staging: &Path, package: &str, target: &str) -> Result<PathBuf> {
    let destination = staging.join(package);
    copy_package_files(root, &destination, package)?;
    let binary_dir = destination.join("bin");
    fs::create_dir_all(&binary_dir)?;
    let source = root.join("target").join(target).join("release").join(BINARY);
    let binary = binary_dir.join(BINARY);
    fs::copy(&source, &binary).with_context(|| format!("cannot copy {}", source.display()))?;
    make_executable(&binary)?;
    sign_macho(&destination, &binary)?;
    verify_macho(&destination, &binary, target)?;
    verify_binary_strings(root, &binary)?;
    let size = fs::metadata(&binary)?.len();
    anyhow::ensure!(size <= MAX_BINARY_BYTES, "{target} binary exceeds the 20 MiB limit");
    Ok(destination)
}

fn stage_root(root: &Path, staging: &Path) -> Result<PathBuf> {
    let destination = staging.join(BINARY);
    copy_package_files(root, &destination, BINARY)?;
    let source = root.join("npm/eas-mail-mcp/bin/eas-mail-mcp.js");
    let binary_dir = destination.join("bin");
    fs::create_dir_all(&binary_dir)?;
    let launcher = binary_dir.join("eas-mail-mcp.js");
    fs::copy(&source, &launcher)?;
    make_executable(&launcher)?;
    Ok(destination)
}

fn copy_package_files(root: &Path, destination: &Path, package: &str) -> Result<()> {
    fs::create_dir_all(destination)?;
    fs::copy(
        root.join("npm").join(package).join("package.json"),
        destination.join("package.json"),
    )?;
    fs::copy(root.join("README.md"), destination.join("README.md"))?;
    fs::copy(root.join("LICENSE-MIT"), destination.join("LICENSE-MIT"))?;
    fs::copy(root.join("LICENSE-APACHE"), destination.join("LICENSE-APACHE"))?;
    Ok(())
}

fn pack_package(package_root: &Path, dist: &Path, package: &str, version: &str) -> Result<PathBuf> {
    let destination = dist.to_string_lossy().into_owned();
    run(
        package_root,
        "npm",
        ["pack", "--ignore-scripts", "--pack-destination", destination.as_str()],
    )?;
    let archive = dist.join(format!("{package}-{version}.tgz"));
    anyhow::ensure!(archive.is_file(), "npm did not create {}", archive.display());
    verify_archive(package_root, &archive)?;
    Ok(archive)
}

fn verify_archive(package_root: &Path, archive: &Path) -> Result<()> {
    let package: serde_json::Value =
        serde_json::from_slice(&fs::read(package_root.join("package.json"))?)?;
    let files = package
        .get("files")
        .and_then(serde_json::Value::as_array)
        .context("npm package files list is missing")?;
    let mut expected = files
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(|path| format!("package/{path}"))
        .collect::<Vec<_>>();
    expected.extend(["package/README.md".into(), "package/package.json".into()]);
    expected.sort();
    let arguments = [OsString::from("-tzf"), archive.as_os_str().to_owned()];
    let mut actual =
        output(package_root, "tar", arguments)?.lines().map(str::to_owned).collect::<Vec<_>>();
    actual.sort();
    anyhow::ensure!(actual == expected, "npm archive contains unexpected files");
    Ok(())
}

fn write_checksums(dist: &Path, archives: &[PathBuf]) -> Result<()> {
    for archive in archives {
        let name = archive.file_name().ok_or_else(|| anyhow::anyhow!("archive name is missing"))?;
        let checksum = format!("{}  {}\n", digest(archive)?, name.to_string_lossy());
        fs::write(dist.join(format!("{}.sha256", name.to_string_lossy())), checksum)?;
    }
    Ok(())
}

fn smoke_install(
    root: &Path,
    dist: &Path,
    root_archive: &Path,
    archives: &[PathBuf],
) -> Result<()> {
    let host_package = if cfg!(target_arch = "aarch64") {
        "eas-mail-mcp-darwin-arm64"
    } else {
        "eas-mail-mcp-darwin-x64"
    };
    let platform_archive = archives
        .iter()
        .find(|path| {
            path.file_name().is_some_and(|name| name.to_string_lossy().starts_with(host_package))
        })
        .ok_or_else(|| anyhow::anyhow!("host platform archive is missing"))?;
    let prefix = dist.join("smoke-prefix");
    fs::create_dir_all(&prefix)?;
    let arguments = install_arguments(&prefix, root_archive, platform_archive);
    run(root, "npm", arguments)?;
    let launcher = prefix.join("bin/eas-mail-mcp");
    let modules = prefix.join("lib/node_modules");
    let other_package = if host_package.ends_with("arm64") {
        "eas-mail-mcp-darwin-x64"
    } else {
        "eas-mail-mcp-darwin-arm64"
    };
    anyhow::ensure!(
        !modules.join(other_package).exists(),
        "npm installed an incompatible native package"
    );
    run(root, launcher.to_string_lossy().as_ref(), ["--version"])?;
    let native_path = output(root, launcher.to_string_lossy().as_ref(), ["native-path"])?;
    anyhow::ensure!(!native_path.trim().ends_with(".js"), "native-path returned the JS launcher");
    anyhow::ensure!(
        native_path.contains(host_package),
        "native-path did not select {host_package}"
    );
    Ok(())
}

fn install_arguments(prefix: &Path, root: &Path, platform: &Path) -> Vec<OsString> {
    [
        "install".into(),
        "--ignore-scripts".into(),
        "--no-audit".into(),
        "--no-fund".into(),
        "--global".into(),
        "--prefix".into(),
        prefix.as_os_str().to_owned(),
        platform.as_os_str().to_owned(),
        root.as_os_str().to_owned(),
    ]
    .into()
}
