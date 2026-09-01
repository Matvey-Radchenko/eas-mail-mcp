#[expect(dead_code, reason = "shared integration-test support is compiled once per test binary")]
mod support;

use chrono::{DateTime, Utc};
use eas_mail_mcp::backend::{AccountBackend as _, BackendEvent};
use eas_mail_protocol::protocol::{
    build_initial_provision, build_item_fetch, build_policy_ack, build_sync,
};
use eas_mail_protocol::{
    CalendarFields, CollectionKind, Command, MeetingResponseChoice, Patch, RequestSafety,
};
use support::{
    calendar_item_response, calendar_uid_change, call, default_policy, mailbox, mutation, options,
    options_with_calendar_writes, provision_response, read, sync_response,
};

#[tokio::test]
async fn gal_backend_uses_only_search_and_preserves_policy_refresh() -> anyhow::Result<()> {
    let request = include_bytes!("../../../fixtures/eas/gal-search/request.wbxml").to_vec();
    let response = include_bytes!("../../../fixtures/eas/gal-search/response.wbxml").to_vec();
    let calls = vec![
        options(),
        read(Command::Search, request.clone(), response.clone()),
        call(Command::Search, request.clone(), Some(123), RequestSafety::RetrySafe, 449, vec![]),
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
        call(Command::Search, request, Some(701), RequestSafety::RetrySafe, 200, response),
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    let first = mailbox.search_people("Alice", 20).await?;
    let repeated = mailbox.search_people("Alice", 20).await?;
    assert_eq!(first, repeated);
    assert_eq!(first.total, 1);
    assert_eq!(
        first.items.first().map(|person| person.email.as_str()),
        Some("alice@example.invalid")
    );
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn occurrence_backend_preserves_original_instance_after_policy_refresh() -> anyhow::Result<()>
{
    let request = include_bytes!("../../../fixtures/eas/meeting-occurrence/request.wbxml").to_vec();
    let response =
        include_bytes!("../../../fixtures/eas/meeting-occurrence/response.wbxml").to_vec();
    let calls = vec![
        options_with_calendar_writes(),
        read(
            Command::Sync,
            build_sync("calendar", "0", CollectionKind::Calendar, 6, 0)?,
            sync_response("calendar-1", 1, false, Vec::new())?,
        ),
        read(
            Command::Sync,
            build_sync("calendar", "calendar-1", CollectionKind::Calendar, 6, 0)?,
            sync_response("calendar-2", 1, false, vec![calendar_uid_change("item", "uid")])?,
        ),
        read(
            Command::ItemOperations,
            build_item_fetch(None, Some("calendar"), Some("item"), 50_000)?,
            calendar_item_response("calendar", "item", "uid")?,
        ),
        mutation(Command::MeetingResponse, request.clone(), response.clone()),
        call(
            Command::MeetingResponse,
            request.clone(),
            Some(123),
            RequestSafety::Mutation,
            449,
            vec![],
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
        call(Command::MeetingResponse, request, Some(701), RequestSafety::Mutation, 200, response),
    ];
    let original = DateTime::parse_from_rfc3339("2026-08-25T10:00:00Z")?.with_timezone(&Utc);
    let source = BackendEvent {
        account_id: "work".into(),
        long_id: String::new(),
        collection_id: Some("calendar".into()),
        server_id: Some("item".into()),
        occurrence_start: Some(original),
        fields: CalendarFields { uid: Patch::Value("uid".into()), ..Default::default() },
    };
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    for _ in 0..2 {
        assert_eq!(
            mailbox.respond_calendar_item(&source, MeetingResponseChoice::Accept).await?.as_deref(),
            Some("item")
        );
    }
    transport.verify_complete()?;
    Ok(())
}
