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

#[test]
fn status_preserves_expired_files_and_clear_is_scoped() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("attachments");
    let cache = AttachmentCache::new(root.clone(), clock(Utc::now()))?;
    let (first, _) = cache.store("one", "token", "one.txt", b"one")?;
    let (second, _) = cache.store("two", "token", "two.txt", b"second")?;
    let expired = AttachmentCache::open(root, clock(Utc::now() + Duration::hours(25)))?;
    let status = expired.status()?;
    assert_eq!((status.files, status.bytes), (2, 9));
    assert_eq!((status.expired_files, status.expired_bytes), (2, 9));
    assert!(first.exists() && second.exists());
    let cleared = expired.clear(Some("one"))?;
    assert_eq!((cleared.removed_files, cleared.removed_bytes), (1, 3));
    assert_eq!((cleared.remaining_files, cleared.remaining_bytes), (1, 6));
    assert!(!first.exists() && second.exists());
    let cleared = expired.clear(None)?;
    assert_eq!((cleared.removed_files, cleared.remaining_files), (1, 0));
    assert_eq!(expired.clear(None)?.removed_files, 0);
    Ok(())
}

#[test]
fn concurrent_downloads_and_clear_share_a_stable_lock() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("attachments");
    let first = AttachmentCache::new(root.clone(), clock(Utc::now()))?;
    let second = AttachmentCache::new(root.clone(), clock(Utc::now()))?;
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let writer_barrier = Arc::clone(&barrier);
    let writer = std::thread::spawn(move || -> Result<()> {
        writer_barrier.wait();
        for index in 0..24 {
            let _ = first.store("work", &format!("token{index}"), "file.txt", b"payload")?;
        }
        Ok(())
    });
    barrier.wait();
    for _ in 0..24 {
        let _ = second.clear(None)?;
    }
    writer.join().map_err(|_| anyhow::anyhow!("cache writer thread failed"))??;
    let guard = second.lock()?;
    let competing = platform::open_private_append(&root.with_extension("lock"))?;
    assert!(competing.try_lock().is_err());
    drop(guard);
    competing.try_lock()?;
    drop(competing);
    let _ = second.clear(None)?;
    assert_eq!(second.status()?.files, 0);
    assert!(root.with_extension("lock").exists());
    Ok(())
}

#[test]
fn cache_status_and_clear_do_not_follow_links() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("attachments");
    let cache = AttachmentCache::new(root.clone(), clock(Utc::now()))?;
    let outside = directory.path().join("outside");
    fs::create_dir(&outside)?;
    let target = outside.join("preserve.txt");
    fs::write(&target, b"preserve")?;
    if !symlink_directory(&outside, &root.join("link"))? {
        return Ok(());
    }
    assert_eq!(cache.status()?.files, 0);
    let _ = cache.clear(None)?;
    assert_eq!(fs::read(target)?, b"preserve");
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
