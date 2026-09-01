#[expect(dead_code, reason = "shared integration-test support is compiled once per test binary")]
mod support;

use chrono::{TimeZone as _, Utc};
use eas_mail_mcp::backend::{
    AccountBackend as _, BackendCalendarMutation, BackendEvent, BackendMail, MailSource,
};
use eas_mail_protocol::protocol::{
    build_calendar_add, build_calendar_change, build_calendar_delete, build_folder_sync,
    build_initial_provision, build_item_fetch, build_meeting_response,
    build_meeting_response_long_id, build_policy_ack, build_send, build_sync,
};
use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{
    CalendarApplication, CalendarAttendee, CalendarFields, CollectionKind, Command,
    MeetingResponseChoice, Patch, RequestSafety,
};

use support::{
    call, default_policy, folder_response, mailbox, mutation, options,
    options_with_calendar_writes, provision_response, read, sync_response,
};

#[tokio::test]
async fn calendar_add_initializes_sync_key_without_loading_events() -> anyhow::Result<()> {
    let item = application()?;
    let calls = vec![
        options_with_calendar_writes(),
        read(Command::FolderSync, build_folder_sync("0")?, folder_response("1", true)?),
        read(
            Command::Sync,
            build_sync("calendar", "0", CollectionKind::Calendar, 6, 0)?,
            sync_response("calendar-1", 1, false, Vec::new())?,
        ),
        mutation(
            Command::Sync,
            build_calendar_add("calendar", "calendar-1", "client-1", &item)?,
            mutation_response("Add", "calendar-2", 1, Some("event-1"))?,
        ),
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    let created = mailbox
        .create_calendar_item(
            "client-1",
            &BackendCalendarMutation { target_collection: None, application: item },
        )
        .await?;
    assert_eq!(created.collection_id.as_deref(), Some("calendar"));
    assert_eq!(created.server_id.as_deref(), Some("event-1"));
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn item_operations_source_is_reused_for_change_response_and_delete() -> anyhow::Result<()> {
    let item = application()?;
    let search_source = BackendEvent {
        occurrence_start: None,
        account_id: "work".into(),
        long_id: "long-1".into(),
        collection_id: None,
        server_id: None,
        fields: CalendarFields::default(),
    };
    let calls = vec![
        options_with_calendar_writes(),
        read(
            Command::ItemOperations,
            build_item_fetch(Some("long-1"), None, None, 50_000)?,
            item_response("calendar", "event-1", "uid-1")?,
        ),
        read(
            Command::Sync,
            build_sync("calendar", "0", CollectionKind::Calendar, 6, 0)?,
            sync_response("calendar-1", 1, false, Vec::new())?,
        ),
        mutation(
            Command::Sync,
            build_calendar_change("calendar", "calendar-1", "event-1", &item)?,
            mutation_response("Change", "calendar-2", 1, None)?,
        ),
        mutation(
            Command::MeetingResponse,
            build_meeting_response("calendar", "event-1", MeetingResponseChoice::Tentative)?,
            meeting_response("event-1", Some("accepted-event"))?,
        ),
        mutation(
            Command::Sync,
            build_calendar_delete("calendar", "calendar-2", "event-1")?,
            mutation_response("Delete", "calendar-3", 1, None)?,
        ),
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    let source = mailbox.resolve_calendar_source(&search_source).await?;
    let updated = mailbox
        .update_calendar_item(
            &source,
            &BackendCalendarMutation { target_collection: None, application: item },
        )
        .await?;
    assert_eq!(updated.server_id.as_deref(), Some("event-1"));
    assert_eq!(
        mailbox.respond_calendar_item(&updated, MeetingResponseChoice::Tentative).await?.as_deref(),
        Some("accepted-event")
    );
    mailbox.delete_calendar_item(&updated).await?;
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn inbox_meeting_response_uses_search_long_id_and_refreshes_policy() -> anyhow::Result<()> {
    let body = build_meeting_response_long_id("long-request", MeetingResponseChoice::Accept)?;
    let calls = vec![
        options_with_calendar_writes(),
        call(
            Command::MeetingResponse,
            body.clone(),
            Some(123),
            RequestSafety::Mutation,
            449,
            Vec::new(),
        ),
        call(
            Command::Provision,
            build_initial_provision()?,
            None,
            RequestSafety::RetrySafe,
            200,
            provision_response(1, Some(700), None)?,
        ),
        call(
            Command::Provision,
            build_policy_ack(700, true)?,
            Some(0),
            RequestSafety::RetrySafe,
            200,
            provision_response(1, Some(701), Some(1))?,
        ),
        call(
            Command::MeetingResponse,
            body,
            Some(701),
            RequestSafety::Mutation,
            200,
            meeting_response("long-request", Some("accepted-event"))?,
        ),
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    let source = BackendMail {
        account_id: "work".into(),
        folder_id: "inbox".into(),
        source: MailSource::LongId("long-request".into()),
        fields: eas_mail_protocol::MailFields::default(),
    };

    assert_eq!(
        mailbox
            .respond_meeting_request(&source.source, MeetingResponseChoice::Accept)
            .await?
            .as_deref(),
        Some("accepted-event")
    );
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn invalid_sync_key_resets_only_calendar_and_scans_paged_metadata() -> anyhow::Result<()> {
    let item = application()?;
    let source = source("old-event", "uid-1");
    let calls = vec![
        options_with_calendar_writes(),
        read(
            Command::Sync,
            build_sync("calendar", "0", CollectionKind::Calendar, 6, 0)?,
            sync_response("stale-key", 1, false, Vec::new())?,
        ),
        mutation(
            Command::Sync,
            build_calendar_change("calendar", "stale-key", "old-event", &item)?,
            mutation_response("Change", "stale-key", 3, None)?,
        ),
        read(
            Command::Sync,
            build_sync("calendar", "0", CollectionKind::Calendar, 6, 0)?,
            sync_response("fresh-1", 1, false, Vec::new())?,
        ),
        read(
            Command::Sync,
            build_sync("calendar", "fresh-1", CollectionKind::Calendar, 6, 0)?,
            sync_response("fresh-2", 1, true, Vec::new())?,
        ),
        read(
            Command::Sync,
            build_sync("calendar", "fresh-2", CollectionKind::Calendar, 6, 0)?,
            sync_response("fresh-3", 1, false, vec![uid_change("new-event", "uid-1")])?,
        ),
        mutation(
            Command::Sync,
            build_calendar_change("calendar", "fresh-3", "new-event", &item)?,
            mutation_response("Change", "fresh-4", 1, None)?,
        ),
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    let updated = mailbox
        .update_calendar_item(
            &source,
            &BackendCalendarMutation { target_collection: None, application: item },
        )
        .await?;
    assert_eq!(updated.server_id.as_deref(), Some("new-event"));
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn calendar_notification_refreshes_policy_before_one_retry() -> anyhow::Result<()> {
    let mime = b"calendar mime".to_vec();
    let calls = vec![
        options_with_calendar_writes(),
        call(
            Command::SendMail,
            build_send("calendar-message", mime.clone())?,
            Some(123),
            RequestSafety::Mutation,
            449,
            Vec::new(),
        ),
        call(
            Command::Provision,
            build_initial_provision()?,
            None,
            RequestSafety::RetrySafe,
            200,
            provision_response(1, Some(700), None)?,
        ),
        call(
            Command::Provision,
            build_policy_ack(700, true)?,
            Some(0),
            RequestSafety::RetrySafe,
            200,
            provision_response(1, Some(701), Some(1))?,
        ),
        call(
            Command::SendMail,
            build_send("calendar-message", mime.clone())?,
            Some(701),
            RequestSafety::Mutation,
            200,
            Vec::new(),
        ),
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    mailbox.send_calendar_message("calendar-message", mime).await?;
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn missing_meeting_response_is_an_optional_feature_failure() -> anyhow::Result<()> {
    let (mailbox, transport) = mailbox(vec![options()], default_policy())?;
    let result = mailbox
        .respond_calendar_item(&source("event-1", "uid-1"), MeetingResponseChoice::Accept)
        .await;
    assert_eq!(
        result.err().map(|error| error.envelope.code),
        Some(eas_mail_mcp::ErrorCode::FeatureUnavailable)
    );
    transport.verify_complete()?;
    Ok(())
}

fn application() -> anyhow::Result<CalendarApplication> {
    let starts_at = Utc
        .with_ymd_and_hms(2026, 8, 24, 9, 0, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("invalid fixture time"))?;
    Ok(CalendarApplication {
        properties: Default::default(),
        time_zone: "AAAA".into(),
        uid: "uid-1".into(),
        dt_stamp: starts_at,
        starts_at,
        ends_at: starts_at + chrono::Duration::hours(1),
        all_day: false,
        subject: "Planning".into(),
        body: "Agenda".into(),
        location: "Room 1".into(),
        reminder_minutes: Some(15),
        busy_status: 2,
        meeting_status: 1,
        response_requested: true,
        attendees: vec![CalendarAttendee {
            email: "guest@example.invalid".into(),
            name: "Guest".into(),
            attendee_type: 1,
            attendee_status: 0,
        }],
    })
}

fn source(server_id: &str, uid: &str) -> BackendEvent {
    BackendEvent {
        occurrence_start: None,
        account_id: "work".into(),
        long_id: String::new(),
        collection_id: Some("calendar".into()),
        server_id: Some(server_id.into()),
        fields: CalendarFields { uid: Patch::Value(uid.into()), ..CalendarFields::default() },
    }
}

fn mutation_response(
    command: &str,
    sync_key: &str,
    status: u16,
    server_id: Option<&str>,
) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("AirSync", "Sync");
    let mut collections = Element::new("AirSync", "Collections");
    let mut collection = Element::new("AirSync", "Collection");
    collection.push(Element::text("AirSync", "Status", "1"));
    collection.push(Element::text("AirSync", "SyncKey", sync_key));
    let mut responses = Element::new("AirSync", "Responses");
    let mut response = Element::new("AirSync", command);
    response.push(Element::text("AirSync", "Status", status.to_string()));
    if let Some(server_id) = server_id {
        response.push(Element::text("AirSync", "ServerId", server_id));
    }
    responses.push(response);
    collection.push(responses);
    collections.push(collection);
    root.push(collections);
    encode(&root)
}

fn item_response(
    collection_id: &str,
    server_id: &str,
    uid: &str,
) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("ItemOperations", "ItemOperations");
    let mut fetch = Element::new("ItemOperations", "Fetch");
    fetch.push(Element::text("ItemOperations", "Status", "1"));
    fetch.push(Element::text("AirSync", "CollectionId", collection_id));
    fetch.push(Element::text("AirSync", "ServerId", server_id));
    let mut properties = Element::new("ItemOperations", "Properties");
    properties.push(Element::text("Calendar", "UID", uid));
    fetch.push(properties);
    root.push(fetch);
    encode(&root)
}

fn meeting_response(
    request_id: &str,
    calendar_id: Option<&str>,
) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("MeetingResponse", "MeetingResponse");
    let mut result = Element::new("MeetingResponse", "Result");
    result.push(Element::text("MeetingResponse", "Status", "1"));
    result.push(Element::text("MeetingResponse", "RequestId", request_id));
    if let Some(calendar_id) = calendar_id {
        result.push(Element::text("MeetingResponse", "CalendarId", calendar_id));
    }
    root.push(result);
    encode(&root)
}

fn uid_change(server_id: &str, uid: &str) -> Element {
    let mut add = Element::new("AirSync", "Add");
    add.push(Element::text("AirSync", "ServerId", server_id));
    let mut application = Element::new("AirSync", "ApplicationData");
    application.push(Element::text("Calendar", "UID", uid));
    add.push(application);
    add
}
