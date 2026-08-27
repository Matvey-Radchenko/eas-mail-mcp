use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

pub(crate) fn standard_directories() -> Option<(PathBuf, PathBuf)> {
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir()?;
        Some((
            home.join("Library/Application Support/EAS Mail MCP"),
            home.join("Library/Caches/EAS Mail MCP"),
        ))
    }
    #[cfg(windows)]
    {
        let local = dirs::data_local_dir()?.join("EAS Mail MCP");
        Some((local.clone(), local))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    None
}

pub(crate) fn ensure_private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || is_reparse_point(&metadata) || !metadata.is_dir() {
        return Err(io::Error::other("managed path is not a private directory"));
    }
    set_directory_permissions(path)
}

pub(crate) fn open_private_new(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    set_file_mode(&mut options);
    options.open(path)
}

pub(crate) fn open_private_append(path: &Path) -> io::Result<File> {
    reject_existing_link(path)?;
    let mut options = OpenOptions::new();
    options.create(true).read(true).append(true);
    set_file_mode(&mut options);
    let file = options.open(path)?;
    protect_file(path)?;
    Ok(file)
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| io::Error::other("path has no parent"))?;
    ensure_private_directory(parent)?;
    atomic_write_in_existing_directory(path, bytes)
}

pub(crate) fn atomic_write_in_existing_directory(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| io::Error::other("path has no parent"))?;
    let parent_metadata = fs::symlink_metadata(parent)?;
    if parent_metadata.file_type().is_symlink()
        || is_reparse_point(&parent_metadata)
        || !parent_metadata.is_dir()
    {
        return Err(io::Error::other("destination directory is not safe"));
    }
    reject_existing_link(path)?;
    let mut temporary = tempfile::Builder::new().prefix(".eas-mail-").tempfile_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    protect_file(path)
}

pub(crate) fn protect_file(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(io::Error::other("managed path is not a private file"));
    }
    set_file_permissions(path)
}

pub(crate) fn reject_existing_link(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link_or_reparse(&metadata) => {
            Err(io::Error::other("managed path must not be a link or reparse point"))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(crate) fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink() || is_reparse_point(metadata)
}

#[cfg(unix)]
fn set_file_mode(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt as _;
    options.mode(0o600);
}

#[cfg(not(unix))]
const fn set_file_mode(_: &mut OpenOptions) {}

#[cfg(unix)]
fn set_directory_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
const fn set_directory_permissions(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_file_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
const fn set_file_permissions(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
const fn is_reparse_point(_: &fs::Metadata) -> bool {
    false
}
