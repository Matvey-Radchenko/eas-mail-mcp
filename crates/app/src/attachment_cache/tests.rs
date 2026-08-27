use chrono::Duration;

use super::*;

#[test]
fn cache_stores_private_files_and_purges_account() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let cache = AttachmentCache::new(directory.path().join("attachments"), clock(Utc::now()))?;
    let (path, expires_at) = cache.store("work/account", "token", "../report.txt", b"payload")?;
    assert_eq!(fs::read(&path)?, b"payload");
    assert_private_permissions(&path)?;
    assert_eq!(expires_at - cache.clock.now(), Duration::hours(24));
    cache.purge_account("work/account")?;
    assert!(!path.exists());
    cache.purge_account("work/account")?;
    Ok(())
}

#[test]
fn startup_prunes_files_symlinks_directories_and_expired_entries() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("attachments");
    fs::create_dir_all(root.join("account/nested"))?;
    fs::write(root.join("loose"), b"remove")?;
    fs::write(root.join("account/old"), b"remove")?;
    if !symlink_file(&root.join("account/old"), &root.join("account/link"))? {
        return Ok(());
    }
    let cache = AttachmentCache::new(root.clone(), clock(Utc::now() + Duration::hours(25)))?;
    assert!(!root.join("loose").exists());
    assert!(!root.join("account/old").exists());
    assert!(!root.join("account/link").exists());
    assert!(!root.join("account/nested").exists());
    cache.purge_account("missing")?;
    Ok(())
}

#[test]
fn symlink_root_is_rejected() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let target = directory.path().join("target");
    fs::create_dir(&target)?;
    let link = directory.path().join("link");
    if !symlink_directory(&target, &link)? {
        return Ok(());
    }
    assert!(AttachmentCache::new(link, clock(Utc::now())).is_err());
    Ok(())
}

#[derive(Debug)]
struct TestClock(DateTime<Utc>);

impl Clock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

fn clock(now: DateTime<Utc>) -> Arc<dyn Clock> {
    Arc::new(TestClock(now))
}

#[cfg(unix)]
fn assert_private_permissions(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    assert_eq!(fs::metadata(path)?.permissions().mode() & 0o777, 0o600);
    assert_eq!(
        fs::metadata(path.parent().ok_or_else(path_error)?)?.permissions().mode() & 0o777,
        0o700
    );
    Ok(())
}

#[cfg(not(unix))]
const fn assert_private_permissions(_: &Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn symlink_file(target: &Path, link: &Path) -> std::io::Result<bool> {
    std::os::unix::fs::symlink(target, link).map(|()| true)
}

#[cfg(windows)]
fn symlink_file(target: &Path, link: &Path) -> std::io::Result<bool> {
    windows_symlink(std::os::windows::fs::symlink_file(target, link))
}

#[cfg(unix)]
fn symlink_directory(target: &Path, link: &Path) -> std::io::Result<bool> {
    std::os::unix::fs::symlink(target, link).map(|()| true)
}

#[cfg(windows)]
fn symlink_directory(target: &Path, link: &Path) -> std::io::Result<bool> {
    windows_symlink(std::os::windows::fs::symlink_dir(target, link))
}

#[cfg(windows)]
fn windows_symlink(result: std::io::Result<()>) -> std::io::Result<bool> {
    match result {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn path_error() -> std::io::Error {
    std::io::Error::other("path has no parent")
}
