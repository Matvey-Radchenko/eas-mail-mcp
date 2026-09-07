use std::collections::BTreeSet;

use anyhow::{Context as _, Result};
use chrono::{DateTime, Datelike as _, Duration, NaiveDate, Utc};
use eas_mail_mcp::{
    CalendarFindRecurringSlotsInput, CalendarFindSlotsInput, CalendarParticipantOptions,
    CalendarParticipantRole, Runtime, ScheduleWeekday, WorkingHoursInput,
};
use serde_json::{Value, json};

use super::fixtures::required;

pub async fn check(runtime: &Runtime, account: &str, email: &str) -> Result<Value> {
    let today = Utc::now().date_naive();
    let one_off = required(
        runtime.calendar_find_slots(input(account, email, today, today + Duration::days(14))).await,
    )?;
    anyhow::ensure!(
        one_off.precision_minutes == 30
            && one_off.buffer_minutes == 15
            && one_off.resolution_complete
            && one_off.participants.len() == 1,
        "one-off precision or resolution contract changed"
    );
    anyhow::ensure!(
        one_off.suggestions.iter().all(|item| item.conflicts.is_empty()),
        "required participant has a one-off conflict"
    );
    // October-November crosses Europe's autumn DST boundary without querying any calendar body.
    let year = today.year() + i32::from(today.month() > 11);
    let from = NaiveDate::from_ymd_opt(year, 10, 1).context("invalid DST start")?;
    let to = NaiveDate::from_ymd_opt(year, 11, 30).context("invalid DST end")?;
    let recurring = required(
        runtime
            .calendar_find_recurring_slots(CalendarFindRecurringSlotsInput {
                schedule: input(account, email, from, to),
                weekday: ScheduleWeekday::Mon,
            })
            .await,
    )?;
    anyhow::ensure!(
        recurring.precision_minutes == 30
            && recurring.buffer_minutes == 15
            && recurring.resolution_complete
            && recurring.participants.len() == 1,
        "recurring precision or resolution contract changed"
    );
    anyhow::ensure!(
        !recurring.suggestions.is_empty(),
        "weekly search produced no candidate patterns"
    );
    let mut offsets = BTreeSet::new();
    let mut occurrence_count = 0;
    let mut unknown = 0;
    for pattern in &recurring.suggestions {
        anyhow::ensure!(
            (8..=9).contains(&pattern.occurrences.len()),
            "two-month weekly occurrence count changed"
        );
        let mut previous = None;
        for item in &pattern.occurrences {
            let start = DateTime::parse_from_rfc3339(&item.starts_at)?;
            let end = DateTime::parse_from_rfc3339(&item.ends_at)?;
            anyhow::ensure!(
                start.format("%H:%M").to_string() == pattern.local_start_time
                    && start.weekday() == chrono::Weekday::Mon
                    && end - start == Duration::minutes(45),
                "weekly wall-clock time or duration changed across DST"
            );
            if let Some(date) = previous {
                anyhow::ensure!(
                    start.date_naive() - date == Duration::days(7),
                    "weekly date is missing"
                );
            }
            previous = Some(start.date_naive());
            offsets.insert(start.offset().local_minus_utc());
            unknown += item
                .conflicts
                .iter()
                .filter(|conflict| {
                    conflict.reasons.iter().any(|reason| {
                        matches!(reason, eas_mail_mcp::CalendarSlotConflictReason::Unknown)
                    })
                })
                .count();
            occurrence_count += 1;
        }
    }
    anyhow::ensure!(offsets.len() == 2, "DST span did not retain both UTC offsets");
    Ok(json!({"precision_minutes":30,"buffer_minutes":15,"own_participant_resolved":true,
        "one_off_suggestions":one_off.suggestions.len(),"weekly_patterns":recurring.suggestions.len(),
        "weekly_occurrences":occurrence_count,"utc_offsets":offsets.len(),"unknown_conflicts":unknown,
        "same_wall_clock_across_dst":true}))
}

fn input(account: &str, email: &str, from: NaiveDate, to: NaiveDate) -> CalendarFindSlotsInput {
    let working_hours = vec![WorkingHoursInput {
        weekdays: vec![
            ScheduleWeekday::Mon,
            ScheduleWeekday::Tue,
            ScheduleWeekday::Wed,
            ScheduleWeekday::Thu,
            ScheduleWeekday::Fri,
        ],
        start: "09:00".into(),
        end: "18:00".into(),
    }];
    CalendarFindSlotsInput {
        account_id: Some(account.into()),
        participants: vec![email.into()],
        date_from: from.to_string(),
        date_to: to.to_string(),
        time_zone: "Europe/Belgrade".into(),
        working_hours: working_hours.clone(),
        duration_minutes: 45,
        allow_tentative: false,
        participant_options: vec![CalendarParticipantOptions {
            input: email.into(),
            role: CalendarParticipantRole::Required,
            time_zone: Some("Europe/Belgrade".into()),
            working_hours: Some(working_hours),
        }],
        buffer_minutes: 15,
        limit: Some(5),
    }
}
