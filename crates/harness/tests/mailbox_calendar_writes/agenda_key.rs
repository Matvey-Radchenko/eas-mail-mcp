use super::*;

#[tokio::test]
async fn creating_after_a_complete_agenda_scan_uses_its_final_key_without_a_rejected_write()
-> anyhow::Result<()> {
    let item = application()?;
    let calls = vec![
        options_with_calendar_writes(),
        read(Command::FolderSync, build_folder_sync("0")?, folder_response("folders", true)?),
        read(
            Command::Sync,
            build_sync("calendar", "0", CollectionKind::Calendar, 6, 0)?,
            sync_response("agenda-1", 1, false, Vec::new())?,
        ),
        read(
            Command::Sync,
            build_sync("calendar", "agenda-1", CollectionKind::Calendar, 6, 0)?,
            sync_response("agenda-2", 1, true, Vec::new())?,
        ),
        read(
            Command::Sync,
            build_sync("calendar", "agenda-2", CollectionKind::Calendar, 6, 0)?,
            sync_response("agenda-final", 1, false, Vec::new())?,
        ),
        mutation(
            Command::Sync,
            build_calendar_add("calendar", "agenda-final", "client-1", &item)?,
            mutation_response("Add", "added", 1, Some("event"))?,
        ),
        read(
            Command::Sync,
            build_sync("calendar", "0", CollectionKind::Calendar, 6, 0)?,
            sync_response("binding-1", 1, false, Vec::new())?,
        ),
        read(
            Command::Sync,
            build_sync("calendar", "binding-1", CollectionKind::Calendar, 6, 0)?,
            sync_response("binding-2", 1, false, vec![calendar_uid_change("event", "uid-1")])?,
        ),
        read(
            Command::ItemOperations,
            build_item_fetch(None, Some("calendar"), Some("event"), 50_000)?,
            calendar_item_response("calendar", "event", "uid-1")?,
        ),
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    assert!(mailbox.scan_calendar_metadata().await?.events.is_empty());
    let created = mailbox
        .create_calendar_item(
            "client-1",
            &BackendCalendarMutation { target_collection: None, application: item },
        )
        .await?;
    assert_eq!(created.server_id.as_deref(), Some("event"));
    transport.verify_complete()?;
    Ok(())
}
