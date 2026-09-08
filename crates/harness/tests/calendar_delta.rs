#[expect(dead_code, reason = "shared wire fixture support")]
mod support;

use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use eas_mail_mcp::{CalendarSyncData, CalendarSyncInput, ErrorCode, Runtime};
use eas_mail_mcp_harness::{
    ExpectedCall, FixedClock, MemoryJournal, ScriptedTransport, SequenceIds,
};
use eas_mail_protocol::protocol::{build_folder_sync, build_sync};
use eas_mail_protocol::wbxml::Element;
use eas_mail_protocol::{CollectionKind, Command};
use serde_json::json;
use support::{
    default_policy, folder_response, mailbox, options_with_calendar, read, sync_response,
};

fn input() -> anyhow::Result<CalendarSyncInput> {
    Ok(serde_json::from_value(
        json!({"account_id":"work", "date_from":"2026-08-01", "date_to":"2026-08-31", "time_zone":"UTC"}),
    )?)
}

fn runtime(
    path: &Path,
    calls: Vec<ExpectedCall>,
) -> anyhow::Result<(Runtime, Arc<ScriptedTransport>)> {
    let (backend, transport) = mailbox(calls, default_policy())?;
    Ok((
        Runtime::with_dependencies(
            vec![Arc::new(backend)],
            Arc::new(MemoryJournal::default()),
            Arc::new(FixedClock::new(chrono::DateTime::UNIX_EPOCH)),
            Arc::new(SequenceIds::default()),
            vec![7; 32],
            path.join("attachments"),
        )?,
        transport,
    ))
}

fn discovery() -> anyhow::Result<Vec<ExpectedCall>> {
    Ok(vec![
        options_with_calendar(),
        read(Command::FolderSync, build_folder_sync("0")?, folder_response("folders", true)?),
    ])
}

fn page(
    key: &str,
    next: &str,
    status: u16,
    more: bool,
    changes: Vec<Element>,
) -> anyhow::Result<ExpectedCall> {
    Ok(read(
        Command::Sync,
        build_sync("calendar", key, CollectionKind::Calendar, 6, 0)?,
        sync_response(next, status, more, changes)?,
    ))
}

fn change(kind: &str, id: &str, subject: Option<&str>, full: bool) -> Element {
    let mut item = Element::new("AirSync", kind);
    item.push(Element::text("AirSync", "ServerId", id));
    if let Some(subject) = subject {
        let mut data = Element::new("AirSync", "ApplicationData");
        data.push(Element::text("Calendar", "Subject", subject));
        if full {
            data.push(Element::text("Calendar", "UID", id));
            data.push(Element::text("Calendar", "StartTime", "20260803T090000Z"));
            data.push(Element::text("Calendar", "EndTime", "20260803T100000Z"));
            data.push(Element::text("Calendar", "Location", "Keep location"));
        }
        item.push(data);
    }
    item
}

fn data(response: eas_mail_mcp::ApiResponse<CalendarSyncData>) -> anyhow::Result<CalendarSyncData> {
    response.data.ok_or_else(|| anyhow::anyhow!("sync failed: {:?}", response.error))
}

#[tokio::test]
async fn no_key_restarts_from_disk_and_fetches_only_changes() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let mut calls = discovery()?;
    calls.extend([
        page("0", "one", 1, false, vec![])?,
        page(
            "one",
            "two",
            1,
            false,
            vec![
                change("Add", "a", Some("Original"), true),
                change("Add", "b", Some("Remove"), true),
            ],
        )?,
    ]);
    let (first, transport) = runtime(directory.path(), calls)?;
    let initial = data(first.calendar_sync(input()?).await)?;
    assert!(initial.complete);
    assert_eq!(initial.items.len(), 2);
    transport.verify_complete()?;
    drop(first);

    let mut calls = discovery()?;
    calls.push(page(
        "two",
        "three",
        1,
        false,
        vec![change("Change", "a", Some(""), false), change("Delete", "b", None, false)],
    )?);
    let (second, transport) = runtime(directory.path(), calls)?;
    let delta = data(second.calendar_sync(input()?).await)?;
    assert_eq!(delta.pages, 1);
    assert_eq!(delta.changes_applied, 2);
    let event = &delta.items.first().context("remaining event")?.event;
    assert_eq!(event.subject, "");
    assert_eq!(event.location, "Keep location");
    let reference = event.event_ref.clone();
    transport.verify_complete()?;
    drop(second);

    let mut calls = discovery()?;
    calls.push(page("three", "four", 1, false, vec![])?);
    let (third, transport) = runtime(directory.path(), calls)?;
    let unchanged = data(third.calendar_sync(input()?).await)?;
    assert_eq!(unchanged.pages, 1);
    assert_eq!(unchanged.changes_applied, 0);
    assert_eq!(unchanged.items.first().context("same event")?.event.event_ref, reference);
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn recovery_retains_old_baseline_and_resumes_saved_initial_pages() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let mut calls = discovery()?;
    calls.extend([
        page("0", "one", 1, false, vec![])?,
        page("one", "two", 1, false, vec![change("Add", "a", Some("Old"), true)])?,
    ]);
    let (first, transport) = runtime(directory.path(), calls)?;
    data(first.calendar_sync(input()?).await)?;
    transport.verify_complete()?;
    drop(first);
    let mut calls = discovery()?;
    calls.push(page("two", "", 3, false, vec![])?);
    let (second, transport) = runtime(directory.path(), calls)?;
    let mut limited = input()?;
    limited.max_pages = Some(1);
    let stale = data(second.calendar_sync(limited.clone()).await)?;
    assert!(stale.ready && !stale.complete);
    assert_eq!(stale.items.first().context("old baseline")?.event.subject, "Old");
    transport.verify_complete()?;
    drop(second);
    let mut calls = discovery()?;
    calls.push(page("0", "new-one", 1, false, vec![])?);
    let (third, transport) = runtime(directory.path(), calls)?;
    let staging = data(third.calendar_sync(limited).await)?;
    assert!(staging.ready && !staging.complete);
    assert_eq!(staging.items.len(), 1);
    transport.verify_complete()?;
    drop(third);
    let mut calls = discovery()?;
    calls.push(page("new-one", "new-two", 1, false, vec![])?);
    let (fourth, transport) = runtime(directory.path(), calls)?;
    let empty = data(fourth.calendar_sync(input()?).await)?;
    assert!(empty.complete);
    assert!(empty.items.is_empty());
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn arbitrary_explicit_key_is_rejected_without_sync() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let (runtime, transport) = runtime(directory.path(), discovery()?)?;
    let mut request = input()?;
    request.sync_key = Some("unrelated".into());
    let response = runtime.calendar_sync(request).await;
    assert_eq!(response.error.context("key mismatch")?.code, ErrorCode::ValidationFailed);
    transport.verify_complete()?;
    Ok(())
}
