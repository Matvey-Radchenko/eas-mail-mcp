use anyhow::Context as _;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use super::*;
use crate::model::{CalendarCreateInput, CalendarScope, CalendarUpdateInput};
use crate::runtime::calendar_prepare;
use crate::runtime::calendar_series::edit::{EditInput, ItemAction};

#[test]
fn repeat_patterns_endings_and_dst_keep_local_identity() -> anyhow::Result<()> {
    for recurrence in [
        json!({"frequency":"daily","end":{"mode":"never"}}),
        json!({"frequency":"weekly","weekdays":["mon","wed"],"interval":2,"end":{"mode":"count","count":4}}),
        json!({"frequency":"monthly","day_of_month":24,"end":{"mode":"until","date":"2027-01-24"}}),
        json!({"frequency":"monthly","weekdays":["mon"],"week_of_month":4,"end":{"mode":"count","count":3}}),
        json!({"frequency":"yearly","month":8,"day_of_month":24,"end":{"mode":"never"}}),
        json!({"frequency":"yearly","month":8,"weekdays":["mon"],"week_of_month":4,"end":{"mode":"count","count":2}}),
    ] {
        let input = create(recurrence)?;
        let prepared = calendar_prepare::create(
            &input,
            DateTime::UNIX_EPOCH,
            "uid".into(),
            "work@example.invalid",
        )?;
        assert_eq!(
            validate_member(
                &prepared.mutation.application,
                prepared.mutation.application.starts_at
            )?,
            1
        );
    }
    let mut input = create(json!({"frequency":"weekly","end":{"mode":"count","count":3}}))?;
    input.schedule = serde_json::from_value(
        json!({"kind":"timed","start":"2026-03-01T09:00:00-05:00","end":"2026-03-01T10:00:00-05:00","time_zone":"America/New_York"}),
    )?;
    let item = calendar_prepare::create(
        &input,
        DateTime::UNIX_EPOCH,
        "uid".into(),
        "work@example.invalid",
    )?
    .mutation
    .application;
    let original = instant("2026-03-08T13:00:00Z")?;
    assert_eq!(validate_member(&item, original)?, 2);
    let instance = selected(&item, original)?;
    assert_eq!(instance.ends_at, instant("2026-03-08T14:00:00Z")?);
    assert!(validate_member(&item, instant("2026-03-08T14:00:00Z")?).is_err());
    Ok(())
}

#[test]
fn malformed_rules_and_ambiguous_times_fail_before_writes() -> anyhow::Result<()> {
    for rule in [
        json!({"frequency":"daily","interval":0,"end":{"mode":"never"}}),
        json!({"frequency":"daily","weekdays":["mon"],"end":{"mode":"never"}}),
        json!({"frequency":"weekly","weekdays":["mon","mon"],"end":{"mode":"never"}}),
        json!({"frequency":"monthly","day_of_month":0,"end":{"mode":"never"}}),
        json!({"frequency":"daily","end":{"mode":"count","count":0}}),
        json!({"frequency":"daily","end":{"mode":"until","date":"2020-01-01"}}),
        json!({"frequency":"daily","end":{"mode":"until","date":"bad"}}),
    ] {
        let input = create(rule)?;
        assert!(
            calendar_prepare::create(
                &input,
                DateTime::UNIX_EPOCH,
                "uid".into(),
                "work@example.invalid"
            )
            .is_err()
        );
    }
    let mut input = create(json!({"frequency":"weekly","end":{"mode":"count","count":3}}))?;
    input.schedule = serde_json::from_value(
        json!({"kind":"timed","start":"2026-03-01T02:30:00-05:00","end":"2026-03-01T03:30:00-05:00","time_zone":"America/New_York"}),
    )?;
    assert!(
        calendar_prepare::create(
            &input,
            DateTime::UNIX_EPOCH,
            "uid".into(),
            "work@example.invalid"
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn exceptions_require_stable_mapping_and_never_cross_neighbors() -> anyhow::Result<()> {
    let input = create(json!({"frequency":"daily","end":{"mode":"count","count":5}}))?;
    let item = calendar_prepare::create(
        &input,
        DateTime::UNIX_EPOCH,
        "uid".into(),
        "work@example.invalid",
    )?
    .mutation
    .application;
    let mut source = source(&item);
    source.occurrence_start = Some(instant("2026-08-25T10:00:00Z")?);
    let mutation: CalendarUpdateInput = serde_json::from_value(
        json!({"event_ref":"unused","scope":"occurrence","subject":"changed","idempotency_key": "11111111-2222-4333-8444-555555555555"}),
    )?;
    let plan = edit::plan(
        &EditInput::Update(Box::new(mutation.clone())),
        &source,
        DateTime::UNIX_EPOCH,
        "work@example.invalid",
    )?;
    let Some(ItemAction::Update(changed)) = plan.steps.first().map(|step| &step.action) else {
        anyhow::bail!("missing update");
    };
    assert_eq!(changed.mutation.application.properties.exceptions.len(), 1);
    let mut next = source.clone();
    next.fields = eas_mail_protocol::CalendarFields::from(&changed.mutation.application);
    let mut invalid = mutation.clone();
    invalid.scope = Some(CalendarScope::Series);
    invalid.schedule = Some(serde_json::from_value(
        json!({"kind":"timed","start":"2026-08-24T12:00:00Z","end":"2026-08-24T13:00:00Z","time_zone":"UTC"}),
    )?);
    assert!(
        edit::plan(
            &EditInput::Update(Box::new(invalid)),
            &next,
            DateTime::UNIX_EPOCH,
            "work@example.invalid"
        )
        .is_err()
    );
    let mut invalid = mutation;
    invalid.schedule = Some(serde_json::from_value(
        json!({"kind":"timed","start":"2026-08-26T11:00:00Z","end":"2026-08-26T12:00:00Z","time_zone":"UTC"}),
    )?);
    assert!(
        edit::plan(
            &EditInput::Update(Box::new(invalid)),
            &source,
            DateTime::UNIX_EPOCH,
            "work@example.invalid"
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn all_day_occurrence_uses_local_dates_across_dst() -> anyhow::Result<()> {
    let mut input = create(json!({"frequency":"daily","end":{"mode":"count","count":5}}))?;
    input.schedule = serde_json::from_value(
        json!({"kind":"all_day","start_date":"2026-03-07","end_date":"2026-03-08","time_zone":"America/New_York"}),
    )?;
    let event = calendar_prepare::create(
        &input,
        DateTime::UNIX_EPOCH,
        "uid".into(),
        "work@example.invalid",
    )?;
    let occurrence =
        prepared(selected(&event.mutation.application, instant("2026-03-08T05:00:00Z")?)?)?;
    assert_eq!(occurrence.mutation.application.ends_at, instant("2026-03-09T04:00:00Z")?);
    assert_eq!(occurrence.all_day_dates.context("dates")?.0.to_string(), "2026-03-08");
    Ok(())
}

#[test]
fn numbered_dates_clamp_to_month_end_without_skipping_occurrences() -> anyhow::Result<()> {
    let cases = [
        (
            json!({"frequency":"monthly","day_of_month":31,"end":{"mode":"count","count":3}}),
            "2028-01-31",
            ["2028-02-29", "2028-03-31"],
            "2028-04-30",
        ),
        (
            json!({"frequency":"monthly","day_of_month":30,"end":{"mode":"count","count":3}}),
            "2027-01-30",
            ["2027-02-28", "2027-03-30"],
            "2027-04-30",
        ),
        (
            json!({"frequency":"yearly","month":2,"day_of_month":29,"end":{"mode":"count","count":3}}),
            "2028-02-29",
            ["2029-02-28", "2030-02-28"],
            "2031-02-28",
        ),
        (
            json!({"frequency":"monthly","weekdays":["mon","tue","wed","thu","fri","sat","sun"],"week_of_month":5,"end":{"mode":"count","count":3}}),
            "2027-01-31",
            ["2027-02-28", "2027-03-31"],
            "2027-04-30",
        ),
    ];
    for (rule, start, members, after_end) in cases {
        let mut input = create(rule)?;
        input.schedule = serde_json::from_value(json!({
            "kind":"timed", "start":format!("{start}T10:00:00Z"),
            "end":format!("{start}T11:00:00Z"), "time_zone":"UTC"
        }))?;
        let item = calendar_prepare::create(
            &input,
            DateTime::UNIX_EPOCH,
            "uid".into(),
            "work@example.invalid",
        )?
        .mutation
        .application;
        for (index, date) in members.iter().enumerate() {
            let original = instant(&format!("{date}T10:00:00Z"))?;
            assert_eq!(validate_member(&item, original)?, u32::try_from(index)? + 2);
            assert_eq!(selected(&item, original)?.starts_at, original);
        }
        assert!(validate_member(&item, instant(&format!("{after_end}T10:00:00Z"))?).is_err());
    }
    Ok(())
}

fn create(recurrence: Value) -> anyhow::Result<CalendarCreateInput> {
    Ok(serde_json::from_value(json!({
        "account_id":"work", "subject":"Test", "idempotency_key":"11111111-2222-4333-8444-555555555555",
        "schedule":{"kind":"timed","start":"2026-08-24T10:00:00Z","end":"2026-08-24T11:00:00Z","time_zone":"UTC"},"recurrence":recurrence
    }))?)
}

fn instant(value: &str) -> anyhow::Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc))
}

fn source(item: &CalendarApplication) -> BackendEvent {
    BackendEvent {
        account_id: "work".into(),
        long_id: String::new(),
        collection_id: Some("calendar".into()),
        server_id: Some("item".into()),
        occurrence_start: None,
        fields: eas_mail_protocol::CalendarFields::from(item),
    }
}
