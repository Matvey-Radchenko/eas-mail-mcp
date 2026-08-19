mod handoff;
mod smoke;

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use sha2::{Digest as _, Sha256};

use crate::command::{output, run, run_env};

pub(super) const BINARY: &str = "eas-mail-mcp";
const MACOS_DEPLOYMENT_TARGET: &str = "14.0";
pub(super) const MAX_BINARY_BYTES: u64 = 20 * 1024 * 1024;

pub(crate) fn build(root: &Path) -> Result<()> {
    ensure_clean(root)?;
    let rustflags = remap_flags(root)?;
    let dist = root.join("dist");
    if dist.exists() {
        fs::remove_dir_all(&dist)?;
    }
    fs::create_dir_all(&dist)?;
    let mut bundles = Vec::with_capacity(2);
    for target in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
        run_env(
            root,
            "cargo",
            ["build", "--release", "--locked", "--target", target, "--package", BINARY],
            &[("MACOSX_DEPLOYMENT_TARGET", MACOS_DEPLOYMENT_TARGET), ("RUSTFLAGS", &rustflags)],
        )?;
        let bundle = create_bundle(root, &dist, target)?;
        smoke::verify(&dist, &bundle, target)?;
        archive(&dist, &bundle)?;
        bundles.push((target, bundle));
    }
    let handoff = handoff::create(root, &dist, &bundles)?;
    smoke::verify_handoff(&dist, &handoff)?;
    archive(&dist, &handoff)?;
    Ok(())
}

fn ensure_clean(root: &Path) -> Result<()> {
    let status = output(root, "git", ["status", "--porcelain=v1", "--untracked-files=all"])?;
    anyhow::ensure!(status.trim().is_empty(), "release bundles require a clean Git worktree");
    Ok(())
}

fn create_bundle(root: &Path, dist: &Path, target: &str) -> Result<PathBuf> {
    let name = format!("{BINARY}-{}-{target}", env!("CARGO_PKG_VERSION"));
    let bundle = dist.join(&name);
    fs::create_dir_all(bundle.join("bin"))?;
    let source = root.join("target").join(target).join("release").join(BINARY);
    let destination = bundle.join("bin").join(BINARY);
    fs::copy(&source, &destination).with_context(|| format!("cannot copy {}", source.display()))?;
    make_executable(&destination)?;
    verify_binary_strings(root, &destination)?;
    sign_macho(&bundle, &destination)?;
    verify_macho(&bundle, &destination, target)?;
    let size = fs::metadata(&destination)?.len();
    anyhow::ensure!(
        size <= MAX_BINARY_BYTES,
        "stripped {target} binary is {size} bytes; limit is {MAX_BINARY_BYTES}"
    );
    fs::copy(root.join("scripts/install.sh"), bundle.join("install.sh"))?;
    fs::copy(root.join("scripts/uninstall.sh"), bundle.join("uninstall.sh"))?;
    fs::copy(root.join("docs/installation.ru.md"), bundle.join("installation.ru.md"))?;
    fs::write(bundle.join("TARGET_ARCH"), format!("{}\n", target_arch(target)))?;
    write_build_metadata(root, &bundle, &destination, target)?;
    make_executable(&bundle.join("install.sh"))?;
    make_executable(&bundle.join("uninstall.sh"))?;
    write_manifest(
        &bundle,
        vec![
            PathBuf::from("TARGET_ARCH"),
            PathBuf::from("BUILD-METADATA.json"),
            PathBuf::from("bin").join(BINARY),
            PathBuf::from("install.sh"),
            PathBuf::from("installation.ru.md"),
            PathBuf::from("uninstall.sh"),
        ],
    )?;
    Ok(bundle)
}

fn archive(dist: &Path, bundle: &Path) -> Result<()> {
    let name = bundle.file_name().ok_or_else(|| anyhow::anyhow!("bundle name is missing"))?;
    let archive = dist.join(format!("{}.tar.gz", name.to_string_lossy()));
    let arguments =
        [std::ffi::OsString::from("-czf"), archive.as_os_str().to_owned(), name.to_owned()];
    run_env(dist, "tar", arguments, &[("COPYFILE_DISABLE", "1")])?;
    let digest = digest(&archive)?;
    let archive_name =
        archive.file_name().ok_or_else(|| anyhow::anyhow!("archive name missing"))?;
    fs::write(
        PathBuf::from(format!("{}.sha256", archive.to_string_lossy())),
        format!("{digest}  {}\n", archive_name.to_string_lossy()),
    )?;
    Ok(())
}

pub(super) fn write_manifest(bundle: &Path, mut files: Vec<PathBuf>) -> Result<()> {
    files.sort();
    let mut manifest = String::new();
    for relative in files {
        writeln!(manifest, "{}  {}", digest(&bundle.join(&relative))?, relative.display())?;
    }
    fs::write(bundle.join("SHA256SUMS"), manifest)?;
    Ok(())
}

pub(super) fn digest(path: &Path) -> Result<String> {
    Ok(Sha256::digest(fs::read(path)?).iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(super) fn sign_macho(root: &Path, binary: &Path) -> Result<()> {
    let binary = binary.to_string_lossy();
    run(root, "codesign", ["--force", "--sign", "-", "--timestamp=none", binary.as_ref()])
}

pub(super) fn verify_macho(root: &Path, binary: &Path, target: &str) -> Result<()> {
    let binary = binary.to_string_lossy();
    output(root, "codesign", ["--verify", "--strict", binary.as_ref()])?;
    let architectures = output(root, "lipo", ["-archs", binary.as_ref()])?;
    anyhow::ensure!(
        architectures.trim() == target_arch(target).trim(),
        "{target} bundle contains unexpected architecture: {}",
        architectures.trim()
    );
    let load_commands = output(root, "otool", ["-l", binary.as_ref()])?;
    anyhow::ensure!(
        load_commands.lines().any(|line| line.trim() == "minos 14.0"),
        "{target} bundle does not target macOS {MACOS_DEPLOYMENT_TARGET}"
    );
    Ok(())
}

pub(super) fn target_arch(target: &str) -> &'static str {
    if target.starts_with("aarch64") { "arm64" } else { "x86_64" }
}

#[cfg(unix)]
pub(super) fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn make_executable(_: &Path) -> Result<()> {
    anyhow::bail!("bundle delivery supports macOS only")
}

pub(super) fn remap_flags(root: &Path) -> Result<String> {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".cargo")))
        .ok_or_else(|| anyhow::anyhow!("cannot determine Cargo home for path remapping"))?;
    Ok(format!(
        "--remap-path-prefix={}=/workspace --remap-path-prefix={}=/cargo",
        root.display(),
        cargo_home.display()
    ))
}

pub(super) fn verify_binary_strings(root: &Path, binary: &Path) -> Result<()> {
    let binary_text = output(root, "strings", [binary.as_os_str()])?;
    let forbidden = [
        root.to_string_lossy().into_owned(),
        ["/", "Users", "/"].concat(),
        "/private/tmp/".into(),
        "EAS_MAIL_PROFILE_BUNDLE".into(),
    ];
    for marker in forbidden {
        anyhow::ensure!(!binary_text.contains(&marker), "release binary leaks a local build path");
    }
    Ok(())
}

fn write_build_metadata(root: &Path, bundle: &Path, binary: &Path, target: &str) -> Result<()> {
    let source_sha = output(root, "git", ["rev-parse", "HEAD"])?;
    let document = serde_json::json!({
        "source_sha": source_sha.trim(),
        "target": target,
        "artifact_sha256": digest(binary)?,
    });
    fs::write(
        bundle.join("BUILD-METADATA.json"),
        format!("{}\n", serde_json::to_string_pretty(&document)?),
    )?;
    Ok(())
}
