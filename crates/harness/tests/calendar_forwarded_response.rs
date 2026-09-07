use anyhow::Context as _;
use eas_mail_mcp::backend::{AccountBackend, BackendEvent, MailSource};
use eas_mail_mcp::{
    CalendarGetInput, CalendarOperationState, CalendarRespondInput, CalendarScope, ErrorCode,
    MailGetInput, MailSearchInput,
};
use eas_mail_mcp_harness::FakeBackend;
use eas_mail_protocol::{CalendarException, CalendarFields, CalendarProperties, Patch};
use serde_json::json;
use std::sync::Arc;
mod series_support;
use series_support::{agenda, create, data, runtime, uuid};

#[tokio::test]
async fn series_and_occurrence_replies_ignore_unwritable_metadata_and_replay_once()
-> anyhow::Result<()> {
    for scope in [CalendarScope::Series, CalendarScope::Occurrence] {
        let backend = Arc::new(FakeBackend::new("work"));
        let (runtime, _dir) = runtime(backend.clone())?;
        data(runtime.calendar_create(create(1)?).await)?;
        let source = received_series(&backend).await?;
        backend.put_calendar_fixture(source.clone())?;
        let reference = agenda(&runtime).await?.get(1).context("occurrence")?.event_ref.clone();
        let detail = data(
            runtime
                .calendar_get(CalendarGetInput {
                    event_ref: reference.clone(),
                    body_limit: Some(1),
                })
                .await,
        )?;
        assert!(detail.can_respond);
        assert!(!detail.can_update && !detail.can_cancel && !detail.can_delete);
        let before = backend.operations()?;
        let input: CalendarRespondInput = serde_json::from_value(
            json!({"event_ref":reference,"scope":scope,"response":"accept","idempotency_key":uuid(2)}),
        )?;
        let result = data(runtime.calendar_respond(input.clone()).await)?;
        assert_eq!(result.status, CalendarOperationState::Succeeded);
        assert_eq!(result.completed_steps, ["meeting_response", "reply_notification"]);
        assert_eq!(
            backend.operations()?.get(before.len()..).context("new operations")?,
            ["calendar_respond_item", "calendar_send"]
        );
        let calls = backend.operations()?;
        assert_eq!(
            data(runtime.calendar_respond(input).await)?.status,
            CalendarOperationState::Succeeded
        );
        assert_eq!(backend.operations()?, calls);
        let ids = backend.calendar_responses()?;
        assert_eq!(ids.len(), 1);
        assert_eq!(ids.first().context("response")?.is_some(), scope == CalendarScope::Occurrence);
        assert_eq!(backend.resolve_calendar_source(&source).await?.fields, source.fields);
    }
    Ok(())
}

#[tokio::test]
async fn forwarded_series_mail_responds_to_organizer_not_sender() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _dir) = runtime(backend.clone())?;
    let source = MailSource::LongId("meeting-request-0".into());
    let mut mail = backend.fetch_mail(&source, 50_000).await?;
    mail.fields.sender = Patch::Value("Forwarder <forwarder@example.invalid>".into());
    let Patch::Value(request) = &mut mail.fields.meeting_request else {
        anyhow::bail!("request");
    };
    request.instance_type = 1;
    backend.put_mail_fixture(mail)?;
    let page = data(
        runtime
            .mail_search(MailSearchInput { query: "meeting-request".into(), ..Default::default() })
            .await,
    )?;
    let summary = page.items.first().context("mail")?;
    assert!(summary.can_respond);
    let detail = data(
        runtime
            .mail_get(MailGetInput { mail_ref: summary.mail_ref.clone(), body_limit: Some(1) })
            .await,
    )?;
    assert!(detail.summary.can_respond);
    let input: CalendarRespondInput = serde_json::from_value(
        json!({"event_ref":summary.mail_ref,"scope":"series","response":"accept","idempotency_key":uuid(3)}),
    )?;
    assert_eq!(
        data(runtime.calendar_respond(input.clone()).await)?.status,
        CalendarOperationState::Succeeded
    );
    let messages = backend.calendar_messages()?;
    let mime = String::from_utf8(messages.first().context("reply")?.clone())?;
    assert!(mime.contains("organizer@example.invalid"));
    assert!(!mime.contains("forwarder@example.invalid"));
    assert_eq!(
        data(runtime.calendar_respond(input).await)?.status,
        CalendarOperationState::Succeeded
    );
    assert_eq!(backend.operations()?, ["calendar_respond_request", "calendar_send"]);
    Ok(())
}

#[tokio::test]
async fn ambiguous_response_and_failed_notification_are_never_repeated() -> anyhow::Result<()> {
    for (operation, code, expected) in [
        ("calendar_respond_item", ErrorCode::OutcomeUnknown, CalendarOperationState::Unknown),
        ("calendar_send", ErrorCode::ProtocolError, CalendarOperationState::Partial),
        ("calendar_send", ErrorCode::OutcomeUnknown, CalendarOperationState::Unknown),
    ] {
        let backend = Arc::new(FakeBackend::new("work"));
        let (runtime, _dir) = runtime(backend.clone())?;
        data(runtime.calendar_create(create(1)?).await)?;
        backend.put_calendar_fixture(received_series(&backend).await?)?;
        let reference = agenda(&runtime).await?.first().context("occurrence")?.event_ref.clone();
        backend.set_operation_failure(Some(operation), code)?;
        let input: CalendarRespondInput = serde_json::from_value(
            json!({"event_ref":reference,"scope":"series","response":"accept","idempotency_key":uuid(4)}),
        )?;
        let result = data(runtime.calendar_respond(input.clone()).await)?;
        assert_eq!(result.status, expected);
        let calls = backend.operations()?;
        backend.set_operation_failure(None, code)?;
        assert_eq!(data(runtime.calendar_respond(input).await)?.status, expected);
        assert_eq!(backend.operations()?, calls);
    }
    Ok(())
}

async fn received_series(backend: &FakeBackend) -> anyhow::Result<BackendEvent> {
    let mut source = backend
        .scan_calendar_metadata()
        .await?
        .events
        .into_iter()
        .find(|event| event.server_id.as_deref() == Some("event-created"))
        .context("series")?;
    source.fields.organizer_email = Patch::Value("organizer@example.invalid".into());
    source.fields.organizer = Patch::Value("Organizer".into());
    source.fields.meeting_status = Patch::Value(3);
    source.fields.response_requested = Patch::Value(true);
    source.fields.body_truncated = Patch::Value(true);
    let properties = source.fields.properties.as_mut().context("properties")?;
    properties.unsupported = true;
    properties.exceptions.push(CalendarException {
        original_start: "2026-08-26T10:00:00Z".parse()?,
        deleted: false,
        fields: CalendarFields {
            body_truncated: Patch::Value(true),
            properties: Some(CalendarProperties { unsupported: true, ..Default::default() }),
            ..Default::default()
        },
    });
    source.fields.exceptions = Patch::Value(
        properties.exceptions.iter().map(eas_mail_protocol::protocol::exception_fields).collect(),
    );
    Ok(source)
}

#[tokio::test]
async fn unsafe_mail_identity_and_occurrence_only_requests_fail_before_writes() -> anyhow::Result<()>
{
    for (kind, expected) in [
        ("organizer", ErrorCode::ProtocolError),
        ("occurrence", ErrorCode::FeatureUnavailable),
        ("exception", ErrorCode::FeatureUnavailable),
    ] {
        let backend = Arc::new(FakeBackend::new("work"));
        let (runtime, _dir) = runtime(backend.clone())?;
        let mut mail =
            backend.fetch_mail(&MailSource::LongId("meeting-request-0".into()), 50_000).await?;
        let Patch::Value(request) = &mut mail.fields.meeting_request else {
            anyhow::bail!("request");
        };
        match kind {
            "organizer" => request.organizer.clear(),
            "occurrence" => request.instance_type = 2,
            _ => request.instance_type = 3,
        }
        backend.put_mail_fixture(mail)?;
        let page = data(
            runtime
                .mail_search(MailSearchInput {
                    query: "meeting-request".into(),
                    ..Default::default()
                })
                .await,
        )?;
        let summary = page.items.first().context("mail")?;
        assert!(!summary.can_respond);
        let input = serde_json::from_value(
            json!({"event_ref":summary.mail_ref,"scope":"series","response":"accept","idempotency_key":uuid(9)}),
        )?;
        let error = runtime.calendar_respond(input).await.error.context("error")?;
        assert_eq!(error.code, expected);
        assert!(error.remediation.is_some());
        assert!(backend.operations()?.is_empty());
    }
    Ok(())
}
