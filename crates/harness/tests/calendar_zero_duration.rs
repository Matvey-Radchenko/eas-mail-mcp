mod series_support;

use std::sync::Arc;

use anyhow::Context as _;
use chrono::{DateTime, Duration, Utc};
use eas_mail_mcp::backend::{AccountBackend as _, BackendEvent};
use eas_mail_mcp::{CalendarGetInput, CalendarScheduleInput, ErrorCode};
use eas_mail_mcp_harness::FakeBackend;
use eas_mail_protocol::{CalendarException, CalendarFields, Patch};
use serde_json::json;
use series_support::{agenda, create, data, runtime, uuid};

#[tokio::test]
async fn zero_length_series_and_exceptions_are_readable_but_not_writable() -> anyhow::Result<()> {
    for zero_master in [true, false] {
        let backend = Arc::new(FakeBackend::new("work"));
        let (runtime, _directory) = runtime(backend.clone())?;
        data(runtime.calendar_create(create(1)?).await)?;
        let mut source = created_source(&backend).await?;
        let original: DateTime<Utc> = "2026-08-25T10:00:00Z".parse()?;
        if zero_master {
            source.fields.ends_at = source.fields.starts_at.clone();
        } else {
            set_exception(&mut source, original, original, false)?;
        }
        backend.put_calendar_fixture(source)?;
        let items = agenda(&runtime).await?;
        assert_eq!(items.len(), 5);
        let occurrence = items.get(1).context("second occurrence")?;
        let before = backend.operations()?;
        let detail = data(
            runtime
                .calendar_get(CalendarGetInput {
                    event_ref: occurrence.event_ref.clone(),
                    body_limit: None,
                })
                .await,
        )?;
        assert_eq!(detail.starts_at.as_deref(), Some("2026-08-25T10:00:00+00:00"));
        assert_eq!(detail.ends_at, detail.starts_at);
        assert_eq!(detail.starts_at, occurrence.starts_at);
        assert_eq!(detail.ends_at, occurrence.ends_at);
        if !zero_master {
            assert_eq!(detail.location, "Shortened occurrence");
            assert_eq!(
                items.first().context("first occurrence")?.ends_at.as_deref(),
                Some("2026-08-24T11:00:00+00:00")
            );
        }
        let update = runtime
            .calendar_update(serde_json::from_value(json!({
                "event_ref":occurrence.event_ref, "scope":"occurrence",
                "subject":"Changed", "idempotency_key":uuid(2)
            }))?)
            .await;
        assert_eq!(update.error.map(|error| error.code), Some(ErrorCode::ValidationFailed));
        assert_eq!(backend.operations()?, before);
    }
    Ok(())
}

#[tokio::test]
async fn zero_length_creation_still_fails_before_backend_writes() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(backend.clone())?;
    let mut input = create(3)?;
    let CalendarScheduleInput::Timed { start, end, .. } = &mut input.schedule else {
        anyhow::bail!("expected timed fixture");
    };
    end.clone_from(start);
    let before = backend.operations()?;
    let result = runtime.calendar_create(input).await;
    assert_eq!(result.error.map(|error| error.code), Some(ErrorCode::ValidationFailed));
    assert_eq!(backend.operations()?, before);
    Ok(())
}

#[tokio::test]
async fn malformed_or_deleted_occurrences_remain_unreadable() -> anyhow::Result<()> {
    for case in ["negative_master", "long_master", "negative_exception", "deleted"] {
        let backend = Arc::new(FakeBackend::new("work"));
        let (runtime, _directory) = runtime(backend.clone())?;
        data(runtime.calendar_create(create(4)?).await)?;
        let reference =
            agenda(&runtime).await?.get(1).context("second occurrence")?.event_ref.clone();
        let mut source = created_source(&backend).await?;
        let original: DateTime<Utc> = "2026-08-25T10:00:00Z".parse()?;
        match case {
            "negative_master" => {
                source.fields.ends_at = Patch::Value(Some("2026-08-24T09:00:00Z".parse()?));
            }
            "long_master" => {
                source.fields.ends_at = Patch::Value(Some(original + Duration::days(366)));
            }
            _ => set_exception(
                &mut source,
                original,
                original - Duration::hours(1),
                case == "deleted",
            )?,
        }
        backend.put_calendar_fixture(source)?;
        let result =
            runtime.calendar_get(CalendarGetInput { event_ref: reference, body_limit: None }).await;
        let expected =
            if case == "deleted" { ErrorCode::SyncStale } else { ErrorCode::ValidationFailed };
        assert_eq!(result.error.map(|error| error.code), Some(expected), "{case}");
    }
    Ok(())
}

#[tokio::test]
async fn zero_length_reads_follow_the_series_timezone_across_dst() -> anyhow::Result<()> {
    let backend = Arc::new(FakeBackend::new("work"));
    let (runtime, _directory) = runtime(backend.clone())?;
    let mut input = create(5)?;
    input.schedule = serde_json::from_value(json!({
        "kind":"timed", "start":"2026-03-27T09:00:00+01:00",
        "end":"2026-03-27T10:00:00+01:00", "time_zone":"Europe/Belgrade"
    }))?;
    data(runtime.calendar_create(input).await)?;
    let mut source = created_source(&backend).await?;
    source.fields.ends_at = source.fields.starts_at.clone();
    backend.put_calendar_fixture(source)?;
    let items = data(
        runtime
            .calendar_search(serde_json::from_value(json!({
                "date_from":"2026-03-28", "date_to":"2026-03-29", "time_zone":"Europe/Belgrade"
            }))?)
            .await,
    )?
    .items;
    assert_eq!(items.len(), 2);
    for (occurrence, expected) in
        items.iter().zip(["2026-03-28T08:00:00+00:00", "2026-03-29T07:00:00+00:00"])
    {
        let detail = data(
            runtime
                .calendar_get(CalendarGetInput {
                    event_ref: occurrence.event_ref.clone(),
                    body_limit: None,
                })
                .await,
        )?;
        assert_eq!(detail.starts_at.as_deref(), Some(expected));
        assert_eq!(detail.ends_at, detail.starts_at);
    }
    Ok(())
}

async fn created_source(backend: &FakeBackend) -> anyhow::Result<BackendEvent> {
    backend
        .scan_calendar_metadata()
        .await?
        .events
        .into_iter()
        .find(|event| event.server_id.as_deref() == Some("event-created"))
        .context("created source")
}

fn set_exception(
    source: &mut BackendEvent,
    original: DateTime<Utc>,
    end: DateTime<Utc>,
    deleted: bool,
) -> anyhow::Result<()> {
    let exception = CalendarException {
        original_start: original,
        deleted,
        fields: CalendarFields {
            ends_at: Patch::Value(Some(end)),
            location: Patch::Value("Shortened occurrence".into()),
            ..Default::default()
        },
    };
    source.fields.exceptions =
        Patch::Value(vec![eas_mail_protocol::protocol::exception_fields(&exception)]);
    source.fields.properties.as_mut().context("lossless Calendar properties")?.exceptions =
        vec![exception];
    Ok(())
}
