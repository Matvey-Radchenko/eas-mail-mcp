use eas_mail_mcp::{JournalFilter, OperationJournal as _, OperationStatus, Paths, SqliteJournal};

use super::*;

async fn find_owned(
    runtime: &Runtime,
    paths: &Paths,
    account: &str,
    create_id: &str,
    failed_id: &str,
) -> Result<Fixture> {
    let journal = SqliteJournal::open(&paths.journal)?;
    let created = journal.inspect(create_id)?.context("create operation is absent")?;
    let failed = journal.inspect(failed_id)?.context("failed operation is absent")?;
    anyhow::ensure!(
        created.record.account_id == account
            && created.record.kind == "calendar_create"
            && created.record.status == OperationStatus::Succeeded
            && failed.record.account_id == account
            && failed.record.kind == "calendar_delete"
            && failed.record.status == OperationStatus::Failed
            && failed.record.completed_steps == 0
            && failed.created_at >= created.created_at
            && Utc::now().timestamp() - created.created_at < 86_400,
        "resume requires recent confirmed create and definitely rejected delete"
    );
    let entries = journal.list(&JournalFilter {
        account_id: Some(account.into()),
        status: None,
        limit: 100,
    })?;
    anyhow::ensure!(
        !entries.iter().any(|entry| entry.created_at >= created.created_at
            && entry.record.kind.starts_with("calendar_")
            && matches!(
                entry.record.status,
                OperationStatus::Pending | OperationStatus::Partial | OperationStatus::Unknown
            )),
        "unresolved calendar operation prevents diagnostic continuation"
    );
    let starts_at = DateTime::from_timestamp(created.created_at, 0)
        .context("invalid creation date")?
        .date_naive()
        .and_hms_opt(10, 0, 0)
        .context("invalid fixture time")?
        .and_utc()
        + Duration::days(21);
    let uid = format!("{}@eas-mail-mcp.local", uuid::Uuid::parse_str(create_id)?);
    let page = required(
        runtime
            .calendar_search(CalendarSearchInput {
                query: Some("EAS Mail MCP calendar self-test".into()),
                date_from: Some(starts_at.date_naive().to_string()),
                date_to: Some((starts_at + Duration::days(15)).date_naive().to_string()),
                time_zone: Some("UTC".into()),
                account_ids: Some(vec![account.into()]),
                limit: Some(100),
            })
            .await,
    )?;
    anyhow::ensure!(!page.results_truncated, "synthetic resume lookup truncated");
    let mut fixture = None;
    for summary in page.items {
        let Some(token) = summary.subject.strip_prefix("EAS Mail MCP calendar self-test ") else {
            continue;
        };
        if uuid::Uuid::parse_str(token).is_err() {
            continue;
        }
        let event = required(
            runtime
                .calendar_get(CalendarGetInput {
                    event_ref: summary.event_ref.clone(),
                    body_limit: Some(1000),
                })
                .await,
        )?;
        if event.uid == uid {
            let found = Fixture {
                account: account.into(),
                subject: summary.subject,
                uid: uid.clone(),
                reference: summary.event_ref,
                starts_at,
            };
            found.check(&event)?;
            fixture = Some(found);
            break;
        }
    }
    fixture.context("exact journal-owned synthetic series was not found")
}

pub async fn run(
    runtime: &Runtime,
    paths: &Paths,
    account: &str,
    create_id: &str,
    failed_id: &str,
    exercise_existing: bool,
) -> Result<()> {
    let fixture = find_owned(runtime, paths, account, create_id, failed_id).await?;
    let original = fixture.occurrences(runtime).await?;
    anyhow::ensure!(
        original.len() == 3
            && original
                .iter()
                .filter(|event| event.location == "Updated synthetic occurrence")
                .count()
                == 1,
        "synthetic series state changed after the definite failure"
    );
    let location = if exercise_existing {
        update_existing(runtime, &fixture, &original).await?
    } else {
        "Updated synthetic occurrence"
    };
    let third = original.get(2).context("missing third occurrence")?;
    fixture.delete(runtime, &third.event_ref, Some(CalendarScope::Occurrence)).await?;
    let remaining = fixture.occurrences(runtime).await?;
    anyhow::ensure!(
        remaining.len() == 2
            && remaining.iter().all(|event| event.starts_at != third.starts_at)
            && remaining.iter().filter(|event| event.location == location).count() == 1,
        "occurrence delete did not preserve the unchanged sibling override"
    );
    super::super::report(
        serde_json::json!({"stage":"resumed_occurrence_delete","remaining_occurrences":2,"sibling_preserved":true}),
    )?;
    fixture.delete(runtime, &fixture.reference, Some(CalendarScope::Series)).await?;
    anyhow::ensure!(
        fixture.occurrences(runtime).await?.is_empty(),
        "deleted synthetic series remains in agenda"
    );
    super::super::report(
        serde_json::json!({"stage":"resumed_series_deleted","remaining_occurrences":0}),
    )
}

pub(super) async fn update_existing(
    runtime: &Runtime,
    fixture: &Fixture,
    original: &[CalendarEvent],
) -> Result<&'static str> {
    let second = original.get(1).context("missing second occurrence")?;
    let mut patch = fixture.patch(&second.event_ref, Some(CalendarScope::Occurrence));
    let location = "Updated twice synthetic occurrence";
    patch.location = Some(location.into());
    let updated = succeeded(runtime.calendar_update(patch).await)?;
    let event = fixture
        .owned(runtime, updated.event_ref.as_deref().context("updated reference absent")?)
        .await?;
    anyhow::ensure!(
        event.location == location
            && event.body == second.body
            && event.starts_at == second.starts_at
            && event.ends_at == second.ends_at,
        "existing exception update changed unrelated properties"
    );
    super::super::report(
        serde_json::json!({"stage":"existing_exception_updated_again","round_trip":true}),
    )?;
    Ok(location)
}
