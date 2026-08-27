mod manifest;

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use crate::command::{output, run, run_env};
use crate::delivery::{
    BINARY, MAX_BINARY_BYTES, digest, make_executable, remap_flags, sign_macho,
    verify_binary_strings, verify_macho, verify_pe_x64,
};

const DEPLOYMENT_TARGET: &str = "14.0";
#[cfg(target_os = "macos")]
const MACOS_PACKAGES: [NativePackage; 2] = [
    NativePackage::new("eas-mail-mcp-darwin-arm64", "aarch64-apple-darwin", BINARY),
    NativePackage::new("eas-mail-mcp-darwin-x64", "x86_64-apple-darwin", BINARY),
];
#[cfg(windows)]
const WINDOWS_PACKAGES: [NativePackage; 1] =
    [NativePackage::new("eas-mail-mcp-win32-x64", "x86_64-pc-windows-msvc", "eas-mail-mcp.exe")];
const PLATFORM_PACKAGE_NAMES: [&str; 3] =
    ["eas-mail-mcp-darwin-arm64", "eas-mail-mcp-darwin-x64", "eas-mail-mcp-win32-x64"];

#[derive(Debug, Clone, Copy)]
struct NativePackage {
    name: &'static str,
    target: &'static str,
    binary: &'static str,
}

impl NativePackage {
    const fn new(name: &'static str, target: &'static str, binary: &'static str) -> Self {
        Self { name, target, binary }
    }
}

pub(crate) fn verify(root: &Path) -> Result<()> {
    manifest::verify(root)
}

pub(crate) fn pack(root: &Path) -> Result<()> {
    verify(root)?;
    let packages = host_packages()?;
    let version = manifest::workspace_version(root)?;
    let dist = root.join("dist/npm");
    let staging = dist.join("staging");
    prepare_directory(&dist)?;
    fs::create_dir_all(&staging)?;

    let rustflags = remap_flags(root)?;
    let mut archives = Vec::with_capacity(packages.len() + 1);
    for package in packages {
        build_target(root, package.target, &rustflags)?;
        let package_root = stage_platform(root, &staging, *package)?;
        archives.push(pack_package(root, &package_root, &dist, package.name, &version)?);
    }

    let root_package = stage_root(root, &staging)?;
    let root_archive = pack_package(root, &root_package, &dist, BINARY, &version)?;
    archives.push(root_archive.clone());
    write_checksums(&dist, &archives)?;
    smoke_install(root, &dist, &root_archive, &archives)?;
    Ok(())
}

pub(crate) fn install_candidate(root: &Path) -> Result<()> {
    verify(root)?;
    let version = manifest::workspace_version(root)?;
    let dist = root.join("dist/npm");
    let host_package = host_package()?;
    let platform_archive = dist.join(format!("{}-{version}.tgz", host_package.name));
    let root_archive = dist.join(format!("{BINARY}-{version}.tgz"));
    anyhow::ensure!(platform_archive.is_file(), "run `cargo xtask npm pack` first");
    anyhow::ensure!(root_archive.is_file(), "run `cargo xtask npm pack` first");

    run(root, "npm", global_install_arguments(&root_archive, &platform_archive))?;
    let prefix_output = output(root, "npm", ["prefix", "--global"])?;
    let prefix = PathBuf::from(prefix_output.trim());
    let launcher = npm_launcher(&prefix);
    run(root, launcher.to_string_lossy().as_ref(), ["--version", "--verbose"])?;
    let native_path = output(root, launcher.to_string_lossy().as_ref(), ["native-path"])?;
    anyhow::ensure!(
        native_path.contains(host_package.name),
        "candidate selected the wrong native package"
    );
    Ok(())
}

fn prepare_directory(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).with_context(|| format!("cannot clear {}", path.display()))?;
    }
    fs::create_dir_all(path).with_context(|| format!("cannot create {}", path.display()))
}

fn build_target(root: &Path, target: &str, rustflags: &str) -> Result<()> {
    if target.ends_with("apple-darwin") {
        return run_env(
            root,
            "cargo",
            ["build", "--release", "--locked", "--target", target, "--package", BINARY],
            &[("MACOSX_DEPLOYMENT_TARGET", DEPLOYMENT_TARGET), ("RUSTFLAGS", rustflags)],
        );
    }
    let rustflags = format!("{rustflags} -C target-feature=+crt-static");
    run_env(
        root,
        "cargo",
        ["build", "--release", "--locked", "--target", target, "--package", BINARY],
        &[("RUSTFLAGS", rustflags.as_str())],
    )
}

fn stage_platform(root: &Path, staging: &Path, package: NativePackage) -> Result<PathBuf> {
    let destination = staging.join(package.name);
    copy_package_files(root, &destination, package.name)?;
    let binary_dir = destination.join("bin");
    fs::create_dir_all(&binary_dir)?;
    let source = root.join("target").join(package.target).join("release").join(package.binary);
    let binary = binary_dir.join(package.binary);
    fs::copy(&source, &binary).with_context(|| format!("cannot copy {}", source.display()))?;
    make_executable(&binary)?;
    if package.target.ends_with("apple-darwin") {
        sign_macho(&destination, &binary)?;
        verify_macho(&destination, &binary, package.target)?;
    } else {
        verify_pe_x64(&binary)?;
    }
    verify_binary_strings(root, &binary)?;
    let size = fs::metadata(&binary)?.len();
    anyhow::ensure!(size <= MAX_BINARY_BYTES, "{} binary exceeds the 20 MiB limit", package.target);
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

fn pack_package(
    root: &Path,
    package_root: &Path,
    dist: &Path,
    package: &str,
    version: &str,
) -> Result<PathBuf> {
    let destination = dist.to_string_lossy().into_owned();
    run(
        package_root,
        "npm",
        ["pack", "--ignore-scripts", "--pack-destination", destination.as_str()],
    )?;
    let archive = dist.join(format!("{package}-{version}.tgz"));
    anyhow::ensure!(archive.is_file(), "npm did not create {}", archive.display());
    verify_archive(root, package_root, &archive, package)?;
    Ok(archive)
}

fn verify_archive(root: &Path, package_root: &Path, archive: &Path, package: &str) -> Result<()> {
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(package_root.join("package.json"))?)?;
    let files = manifest
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
    let audit_root = archive
        .parent()
        .ok_or_else(|| anyhow::anyhow!("npm archive parent is missing"))?
        .join("audit")
        .join(package);
    prepare_directory(&audit_root)?;
    run(
        package_root,
        "tar",
        ["-xzf".as_ref(), archive.as_os_str(), "-C".as_ref(), audit_root.as_os_str()],
    )?;
    crate::public_audit::audit_tree(root, &audit_root, "unpacked npm package")?;
    fs::remove_dir_all(&audit_root)?;
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
    let host_package = host_package()?;
    let platform_archive = archives
        .iter()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(host_package.name))
        })
        .ok_or_else(|| anyhow::anyhow!("host platform archive is missing"))?;
    let prefix = dist.join("smoke-prefix");
    fs::create_dir_all(&prefix)?;
    let arguments = install_arguments(&prefix, root_archive, platform_archive);
    run(root, "npm", arguments)?;
    let launcher = npm_launcher(&prefix);
    let modules = npm_modules(&prefix);
    for package in PLATFORM_PACKAGE_NAMES {
        if package != host_package.name {
            anyhow::ensure!(
                !modules.join(package).exists(),
                "npm installed incompatible native package {package}"
            );
        }
    }
    run(root, launcher.to_string_lossy().as_ref(), ["--version"])?;
    let native_path = output(root, launcher.to_string_lossy().as_ref(), ["native-path"])?;
    anyhow::ensure!(!native_path.trim().ends_with(".js"), "native-path returned the JS launcher");
    anyhow::ensure!(
        native_path.contains(host_package.name),
        "native-path did not select {}",
        host_package.name
    );
    Ok(())
}

fn host_package() -> Result<NativePackage> {
    let target = host_target()?;
    host_packages()?
        .iter()
        .copied()
        .find(|package| package.target == target)
        .ok_or_else(|| anyhow::anyhow!("npm packaging does not support host target {target}"))
}

#[cfg(target_os = "macos")]
const fn host_packages() -> Result<&'static [NativePackage]> {
    Ok(&MACOS_PACKAGES)
}

#[cfg(windows)]
fn host_packages() -> Result<&'static [NativePackage]> {
    anyhow::ensure!(cfg!(target_arch = "x86_64"), "npm packaging supports Windows x64 only");
    Ok(&WINDOWS_PACKAGES)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn host_packages() -> Result<&'static [NativePackage]> {
    anyhow::bail!("npm packaging requires macOS or Windows x64")
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const fn host_target() -> Result<&'static str> {
    Ok("aarch64-apple-darwin")
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const fn host_target() -> Result<&'static str> {
    Ok("x86_64-apple-darwin")
}

#[cfg(all(windows, target_arch = "x86_64"))]
const fn host_target() -> Result<&'static str> {
    Ok("x86_64-pc-windows-msvc")
}

#[cfg(not(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "x86_64"),
    all(windows, target_arch = "x86_64")
)))]
fn host_target() -> Result<&'static str> {
    anyhow::bail!("npm packaging does not support this host architecture")
}

#[cfg(windows)]
fn npm_launcher(prefix: &Path) -> PathBuf {
    prefix.join("eas-mail-mcp.cmd")
}

#[cfg(not(windows))]
fn npm_launcher(prefix: &Path) -> PathBuf {
    prefix.join("bin/eas-mail-mcp")
}

#[cfg(windows)]
fn npm_modules(prefix: &Path) -> PathBuf {
    prefix.join("node_modules")
}

#[cfg(not(windows))]
fn npm_modules(prefix: &Path) -> PathBuf {
    prefix.join("lib/node_modules")
}

fn global_install_arguments(root: &Path, platform: &Path) -> Vec<OsString> {
    [
        "install".into(),
        "--ignore-scripts".into(),
        "--no-audit".into(),
        "--no-fund".into(),
        "--global".into(),
        platform.as_os_str().to_owned(),
        root.as_os_str().to_owned(),
    ]
    .into()
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
