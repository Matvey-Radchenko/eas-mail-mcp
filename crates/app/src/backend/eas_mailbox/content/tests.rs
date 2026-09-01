use std::collections::BTreeMap;

use chrono::{TimeZone as _, Utc};

use super::*;
use crate::backend::eas_mailbox::session::CollectionState;

#[test]
fn snapshot_sort_keys_preserve_missing_and_present_timestamps() -> anyhow::Result<()> {
    let timestamp = Utc
        .with_ymd_and_hms(2026, 8, 14, 12, 0, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("fixture timestamp is invalid"))?;
    let mut mail = MailFields::default();
    assert_eq!(received(&mail), None);
    mail.received_at = Patch::Value(Some(timestamp));
    assert_eq!(received(&mail), Some(timestamp));

    Ok(())
}

#[test]
fn source_parts_supports_collection_and_search_references() {
    let item = MailSource::Item { folder_id: "inbox".into(), server_id: "mail-1".into() };
    assert_eq!(source_parts(&item), (None, Some("inbox"), Some("mail-1")));

    let search = MailSource::LongId("long-1".into());
    assert_eq!(source_parts(&search), (Some("long-1"), None, None));
}

#[test]
fn missing_session_policy_fails_closed() {
    let state = SessionState {
        capabilities: None,
        policy_key: 0,
        policy: None,
        folder_sync_key: "0".into(),
        folders: BTreeMap::new(),
        collections: BTreeMap::<String, CollectionState>::new(),
        calendar_bindings: BTreeMap::new(),
    };
    let error = policy(&state).err();
    assert!(error.is_some_and(|value| value.envelope.code == ErrorCode::ProtocolError));
}
