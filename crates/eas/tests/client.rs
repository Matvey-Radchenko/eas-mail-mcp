mod support;

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{TimeZone as _, Utc};
use eas_mail_protocol::protocol::{ComposeSource, evaluate_policy};
use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{
    CalendarApplication, CollectionKind, Command, EasClient, EasError, MeetingResponseChoice,
    RequestSafety,
};

use support::*;

#[tokio::test]
async fn options_validates_http_version_and_commands() -> anyhow::Result<()> {
    let mut calendar_headers = headers();
    calendar_headers.insert(
        "ms-asprotocolcommands".into(),
        "Provision,FolderSync,Sync,Search,ItemOperations,SendMail,SmartReply,SmartForward,MeetingResponse,ResolveRecipients"
            .into(),
    );
    let capabilities = EasClient::new(boundary(QueueTransport::with_options(response(
        200,
        Vec::new(),
        calendar_headers,
    ))))
    .options()
    .await?;
    assert!(capabilities.supports_writes());
    assert!(capabilities.supports(Command::ResolveRecipients));
    assert!(capabilities.supports_personal_calendar_writes());
    assert!(capabilities.supports_meeting_lifecycle());

    let read_only = BTreeMap::from([
        ("ms-asprotocolversions".into(), "14.1".into()),
        ("ms-asprotocolcommands".into(), "Provision,FolderSync,Sync,Search,ItemOperations".into()),
    ]);
    let capabilities = EasClient::new(boundary(QueueTransport::with_options(response(
        200,
        Vec::new(),
        read_only,
    ))))
    .options()
    .await?;
    assert!(!capabilities.supports_writes());
    assert!(!capabilities.supports(Command::ResolveRecipients));
    assert!(capabilities.supports_personal_calendar_writes());
    assert!(!capabilities.supports_meeting_lifecycle());

    let mut unsupported_version = headers();
    unsupported_version.insert("ms-asprotocolversions".into(), "12.1".into());
    for response in [
        response(401, Vec::new(), headers()),
        response(500, Vec::new(), headers()),
        response(200, Vec::new(), unsupported_version),
        response(
            200,
            Vec::new(),
            BTreeMap::from([
                ("ms-asprotocolversions".into(), "14.1".into()),
                ("ms-asprotocolcommands".into(), "Provision,Sync".into()),
            ]),
        ),
    ] {
        assert!(
            EasClient::new(boundary(QueueTransport::with_options(response)))
                .options()
                .await
                .is_err()
        );
    }
    Ok(())
}

#[tokio::test]
async fn calendar_client_methods_preserve_command_safety_and_results() -> anyhow::Result<()> {
    let transport = Arc::new(QueueTransport::with_commands(vec![
        response(200, search_empty()?, BTreeMap::new()),
        response(200, item_response()?, BTreeMap::new()),
        response(200, item_response()?, BTreeMap::new()),
        response(200, mutation_response()?, BTreeMap::new()),
        response(200, mutation_response()?, BTreeMap::new()),
        response(200, mutation_response()?, BTreeMap::new()),
        response(200, meeting_response()?, BTreeMap::new()),
    ]));
    let client = EasClient::new(transport.clone());
    assert!(client.search_calendar(5, "Planning", 0, 20).await?.items.is_empty());
    assert_eq!(client.fetch_calendar_item(5, "long", 12_000).await?.server_id, None);
    assert_eq!(
        client
            .fetch_calendar_source(5, None, Some("calendar"), Some("event"), 50_001)
            .await?
            .server_id
            .as_deref(),
        None
    );
    let item = calendar_application()?;
    assert_eq!(client.calendar_add(5, "calendar", "1", "client", &item).await?.status, 1);
    assert_eq!(client.calendar_change(5, "calendar", "event", "2", &item).await?.status, 1);
    assert_eq!(client.calendar_delete(5, "calendar", "event", "3").await?.status, 1);
    let response =
        client.meeting_response(5, "calendar", "request", MeetingResponseChoice::Accept).await?;
    assert_eq!(response.calendar_id.as_deref(), Some("calendar-event"));

    let calls = transport.calls()?;
    assert_eq!(calls.iter().filter(|call| call.safety == RequestSafety::RetrySafe).count(), 3);
    assert_eq!(calls.iter().filter(|call| call.safety == RequestSafety::Mutation).count(), 4);
    Ok(())
}

#[tokio::test]
async fn provision_wipe_ack_purges_transport_secrets() -> anyhow::Result<()> {
    for account_only in [false, true] {
        let transport = Arc::new(QueueTransport::with_commands(vec![
            response(200, wipe_response(account_only)?, BTreeMap::new()),
            response(204, Vec::new(), BTreeMap::new()),
        ]));
        let client = EasClient::new(transport.clone());
        assert!(matches!(client.provision().await, Err(EasError::AccountRemoteWipe)));
        assert!(transport.was_purged());
    }

    let failed_ack = Arc::new(QueueTransport::with_commands(vec![
        response(200, wipe_response(false)?, BTreeMap::new()),
        response(500, Vec::new(), BTreeMap::new()),
    ]));
    let client = EasClient::new(failed_ack.clone());
    assert!(matches!(client.provision().await, Err(EasError::AccountRemoteWipe)));
    assert!(failed_ack.was_purged());
    Ok(())
}

#[tokio::test]
async fn folder_and_sync_map_statuses_and_empty_incremental_response() -> anyhow::Result<()> {
    let client = EasClient::new(boundary(QueueTransport::with_commands(vec![
        response(200, folder_response(1, "2")?, BTreeMap::new()),
        response(200, folder_response(9, "")?, BTreeMap::new()),
        response(200, folder_response(7, "")?, BTreeMap::new()),
        response(200, Vec::new(), BTreeMap::new()),
        response(200, sync_response(3, "old")?, BTreeMap::new()),
        response(200, sync_response(7, "old")?, BTreeMap::new()),
    ])));
    assert_eq!(client.folder_sync(1, "0").await?.sync_key, "2");
    assert!(matches!(client.folder_sync(1, "2").await, Err(EasError::InvalidFolderSyncKey)));
    assert!(matches!(client.folder_sync(1, "2").await, Err(EasError::Protocol(_))));

    let empty = client.sync(1, "inbox", "2", CollectionKind::Mail, 5, 500).await?;
    assert_eq!(empty.sync_key, "2");
    assert!(empty.changes.is_empty());
    assert!(matches!(
        client.sync(1, "inbox", "old", CollectionKind::Mail, 5, 500).await,
        Err(EasError::InvalidSyncKey)
    ));
    assert!(matches!(
        client.sync(1, "inbox", "old", CollectionKind::Mail, 5, 500).await,
        Err(EasError::Protocol(_))
    ));
    Ok(())
}

#[tokio::test]
async fn read_and_write_methods_parse_success_and_policy_refresh() -> anyhow::Result<()> {
    let commands = vec![
        response(200, search_empty()?, BTreeMap::new()),
        response(200, item_response()?, BTreeMap::new()),
        response(200, attachment_response()?, BTreeMap::new()),
        response(200, mutation_response()?, BTreeMap::new()),
        response(204, Vec::new(), BTreeMap::new()),
        response(200, compose_response(2)?, BTreeMap::new()),
        response(200, compose_response(3)?, BTreeMap::new()),
        response(449, Vec::new(), BTreeMap::new()),
    ];
    let transport = Arc::new(QueueTransport::with_commands(commands));
    let client = EasClient::new(transport.clone());
    assert!(client.search(5, "query", 0, 5, 500).await?.is_empty());
    assert_eq!(
        client.fetch_item(5, Some("long"), None, None, 99_999).await?.fields.subject,
        eas_mail_protocol::Patch::Value("Full".into())
    );
    assert_eq!(client.fetch_attachment(5, "file").await?, b"bytes");
    assert_eq!(client.mark_read(5, "inbox", "mail", "9", true).await?.status, 1);
    assert_eq!(client.send(5, "client", b"mime".to_vec()).await?.status, 1);
    assert_eq!(
        client
            .smart_compose(5, false, "client", ComposeSource::LongId("long"), b"mime".to_vec())
            .await?
            .status,
        2
    );
    assert_eq!(
        client
            .smart_compose(
                5,
                true,
                "client",
                ComposeSource::Item { folder_id: "inbox", item_id: "mail" },
                b"mime".to_vec()
            )
            .await?
            .status,
        3
    );
    assert!(matches!(
        client.search(5, "query", 0, 5, 500).await,
        Err(EasError::PolicyRefreshRequired)
    ));

    let calls = transport.calls()?;
    assert_eq!(calls.iter().filter(|call| call.safety == RequestSafety::Mutation).count(), 4);
    Ok(())
}

#[tokio::test]
async fn provision_persists_only_accepted_policy() -> anyhow::Result<()> {
    let accepted = EasClient::new(boundary(QueueTransport::with_commands(vec![
        response(200, provision_response(1, Some(10), None, false)?, BTreeMap::new()),
        response(200, provision_response(1, Some(11), Some(1), false)?, BTreeMap::new()),
    ])))
    .provision()
    .await?;
    assert_eq!(accepted.key, 11);
    assert_eq!(accepted.decision, evaluate_policy(&BTreeMap::new()));

    let unsupported = EasClient::new(boundary(QueueTransport::with_commands(vec![
        response(200, provision_response(1, Some(10), None, true)?, BTreeMap::new()),
        response(200, provision_response(1, Some(11), Some(1), false)?, BTreeMap::new()),
    ])))
    .provision()
    .await;
    assert!(matches!(unsupported, Err(EasError::UnsupportedDevicePolicy(_))));
    Ok(())
}

#[tokio::test]
async fn provision_rejects_bad_statuses_and_missing_keys() -> anyhow::Result<()> {
    for responses in [
        vec![response(200, provision_response(2, Some(10), None, false)?, BTreeMap::new())],
        vec![response(200, provision_response(1, None, None, false)?, BTreeMap::new())],
        vec![
            response(200, provision_response(1, Some(10), None, false)?, BTreeMap::new()),
            response(200, provision_response(1, Some(11), Some(2), false)?, BTreeMap::new()),
        ],
        vec![
            response(200, provision_response(1, Some(10), None, false)?, BTreeMap::new()),
            response(200, provision_response(1, None, Some(1), false)?, BTreeMap::new()),
        ],
    ] {
        assert!(
            EasClient::new(boundary(QueueTransport::with_commands(responses)))
                .provision()
                .await
                .is_err()
        );
    }
    Ok(())
}

fn calendar_application() -> anyhow::Result<CalendarApplication> {
    let starts_at = Utc
        .with_ymd_and_hms(2026, 8, 24, 9, 0, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("invalid Calendar fixture time"))?;
    Ok(CalendarApplication {
        properties: Default::default(),
        time_zone: "AAAA".into(),
        uid: "uid-client".into(),
        dt_stamp: starts_at,
        starts_at,
        ends_at: starts_at + chrono::Duration::hours(1),
        all_day: false,
        subject: "Planning".into(),
        body: String::new(),
        location: String::new(),
        reminder_minutes: None,
        busy_status: 2,
        meeting_status: 0,
        response_requested: false,
        attendees: Vec::new(),
    })
}

fn meeting_response() -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("MeetingResponse", "MeetingResponse");
    let mut result = Element::new("MeetingResponse", "Result");
    result.push(Element::text("MeetingResponse", "Status", "1"));
    result.push(Element::text("MeetingResponse", "RequestId", "request"));
    result.push(Element::text("MeetingResponse", "CalendarId", "calendar-event"));
    root.push(result);
    encode(&root)
}
