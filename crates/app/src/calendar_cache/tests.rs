use super::*;
use eas_mail_protocol::SyncChange as Change;

fn page(key: &str, more: bool, changes: Vec<Change>) -> SyncPage {
    SyncPage {
        account_status: 1,
        collection_status: 1,
        sync_key: key.into(),
        more_available: more,
        changes,
    }
}

fn add(subject: &str) -> Change {
    Change {
        kind: ChangeKind::Add,
        server_id: "event".into(),
        data: ChangeData::Calendar(CalendarFields {
            subject: Patch::Value(subject.into()),
            location: Patch::Value("Room".into()),
            ..CalendarFields::default()
        }),
    }
}

#[test]
fn partial_rebuild_and_soft_delete() -> anyhow::Result<()> {
    let mut collection = Collection::default();
    collection.restart(6);
    collection.apply(page("one", false, vec![]))?;
    assert!(collection.more_available);
    collection.apply(page("two", false, vec![add("Old")]))?;
    collection.restart(6);
    collection.apply(page("three", true, vec![add("New")]))?;
    assert_eq!(collection.current.as_ref().map(|s| s.key.as_str()), Some("two"));
    collection.apply(page("four", false, vec![]))?;
    assert_eq!(collection.current.as_ref().map(|s| s.key.as_str()), Some("four"));
    collection.apply(page(
        "five",
        false,
        vec![Change {
            kind: ChangeKind::SoftDelete,
            server_id: "event".into(),
            data: ChangeData::None,
        }],
    ))?;
    assert!(collection.current.as_ref().is_some_and(|s| s.events.is_empty()));
    Ok(())
}

#[test]
fn missing_fields_preserve_and_empty_fields_clear() {
    let mut fields = CalendarFields {
        subject: Patch::Value("Old".into()),
        location: Patch::Value("Room".into()),
        attendees: Patch::Value(vec![]),
        ..CalendarFields::default()
    };
    merge_fields(
        &mut fields,
        CalendarFields { subject: Patch::Value(String::new()), ..CalendarFields::default() },
    );
    assert!(matches!(fields.subject, Patch::Value(ref s) if s.is_empty()));
    assert!(matches!(fields.location, Patch::Value(ref s) if s == "Room"));
    assert!(matches!(fields.attendees, Patch::Value(ref s) if s.is_empty()));
}

#[cfg(unix)]
#[test]
fn private_atomic_cache_rejects_links_and_keeps_previous_commit() -> anyhow::Result<()> {
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("calendar-sync");
    let cache = CalendarCache::new(root.clone());
    let original = cache.load("work", "binding")?;
    cache.save("work", &original)?;
    let path = root.join("work.json");
    assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    assert_eq!(fs::metadata(&root)?.permissions().mode() & 0o777, 0o700);
    assert!(cache.load("work", "other").is_err());
    let saved = root.join("previous.json");
    fs::rename(&path, &saved)?;
    symlink(&saved, &path)?;
    assert!(cache.save("work", &original).is_err());
    assert!(cache.load("work", "binding").is_err());
    let unchanged: AccountCache = serde_json::from_slice(&fs::read(saved)?)?;
    assert_eq!(unchanged.identity, "binding");
    Ok(())
}
