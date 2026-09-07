use super::tests::{exact, free_pages, input};
use super::*;
use crate::model::{
    CalendarFindRecurringSlotsInput, CalendarParticipantOptions, CalendarSlotConflictReason,
    ScheduleWeekday, WorkingHoursInput,
};
use eas_mail_protocol::FreeBusyStatus;

fn missing() -> AppError {
    super::super::state_error()
}

#[test]
fn unrelated_dst_day_does_not_reject_weekly_or_personal_hours() -> Result<()> {
    for (from, to) in [("2026-03-23", "2026-03-30"), ("2026-10-19", "2026-10-26")] {
        let mut schedule = input();
        schedule.date_from = from.into();
        schedule.date_to = to.into();
        schedule.time_zone = "Europe/Belgrade".into();
        schedule.working_hours.push(WorkingHoursInput {
            weekdays: vec![ScheduleWeekday::Sun],
            start: "02:30".into(),
            end: "04:00".into(),
        });
        let input = CalendarFindRecurringSlotsInput { schedule, weekday: ScheduleWeekday::Mon };
        let plan = build(&input.schedule, Some(input.weekday))?;
        assert_eq!(plan.queries.len(), 2);
        let records = prepare(&input.schedule.participants, free_pages(&input.schedule, &plan))?;
        let output = find_recurring("work".into(), &plan, &records, &input)?;
        assert!(output.suggestions.iter().all(|value| value.required_available_occurrences == 2));
        assert_eq!(output.suggestions.first().ok_or_else(missing)?.local_start_time, "09:00");
    }
    Ok(())
}

#[test]
fn midnight_exclusive_end_does_not_materialize_next_personal_dst_day() -> Result<()> {
    let mut input = input();
    input.date_from = "2026-03-28".into();
    input.date_to = input.date_from.clone();
    input.working_hours = vec![WorkingHoursInput {
        weekdays: vec![ScheduleWeekday::Sat],
        start: "22:00".into(),
        end: "23:00".into(),
    }];
    input.participant_options.push(CalendarParticipantOptions {
        input: input.participants.first().ok_or_else(missing)?.clone(),
        role: CalendarParticipantRole::Required,
        time_zone: Some("Europe/Belgrade".into()),
        working_hours: Some(vec![
            WorkingHoursInput {
                weekdays: vec![ScheduleWeekday::Sat],
                start: "23:00".into(),
                end: "23:59".into(),
            },
            WorkingHoursInput {
                weekdays: vec![ScheduleWeekday::Sun],
                start: "02:30".into(),
                end: "04:00".into(),
            },
        ]),
    });
    let plan = build(&input, None)?;
    let records = prepare(&input.participants, free_pages(&input, &plan))?;
    assert!(!find("work".into(), &plan, &records, &input)?.suggestions.is_empty());
    Ok(())
}

#[test]
fn padded_free_intervals_remain_continuous_across_shifted_page_boundary() -> Result<()> {
    let mut input = input();
    input.date_from = "2026-03-23".into();
    input.date_to = "2026-03-30".into();
    input.time_zone = "Europe/Belgrade".into();
    input.working_hours.first_mut().ok_or_else(missing)?.end = "09:45".into();
    input.duration_minutes = 15;
    input.buffer_minutes = 15;
    let plan = build(&input, None)?;
    let first = plan.queries.first().ok_or_else(missing)?;
    let second = plan.queries.get(1).ok_or_else(missing)?;
    assert_eq!(first.end, second.start);
    assert_eq!(second.end - second.start, Duration::minutes(30));
    assert_eq!(first.end - first.start, Duration::days(7) - Duration::minutes(15));
    let records = prepare(&input.participants, free_pages(&input, &plan))?;
    let output = find("work".into(), &plan, &records, &input)?;
    assert_eq!(output.suggestions.len(), 6);
    assert_eq!(output.windows.len(), 2);
    assert!(output.suggestions.iter().all(|value| value.conflicts.is_empty()));
    Ok(())
}

#[test]
fn scheduling_limits_accept_boundaries_and_reject_excess_before_queries() -> Result<()> {
    let mut input = input();
    input.date_to = "2026-09-02".into();
    input.buffer_minutes = 120;
    input.duration_minutes = 480;
    input.limit = Some(50);
    input.participants = (0..20).map(|value| format!("person-{value}")).collect();
    input.participant_options.clear();
    let hours = input.working_hours.first().ok_or_else(missing)?.clone();
    input.working_hours = vec![hours; 32];
    let plan = build(&input, None)?;
    assert!(plan.queries.len() <= 5);
    assert!(plan.queries.iter().all(|range| {
        let duration = range.end - range.start;
        duration >= Duration::minutes(30) && duration <= Duration::days(7)
    }));
    input.participants.push("too-many".into());
    assert!(build(&input, None).is_err());
    input.participants.pop();
    input.working_hours.push(input.working_hours.first().ok_or_else(missing)?.clone());
    assert!(build(&input, None).is_err());
    input.working_hours.pop();
    for buffer in [1, 14, 121, 135] {
        input.buffer_minutes = buffer;
        assert!(build(&input, None).is_err());
    }
    input.buffer_minutes = 0;
    for duration in [0, 14, 481, 495] {
        input.duration_minutes = duration;
        assert!(build(&input, None).is_err());
    }
    input.duration_minutes = 15;
    for limit in [0, 51] {
        input.limit = Some(limit);
        assert!(build(&input, None).is_err());
    }
    input.limit = Some(1);
    assert!(build(&input, None).is_ok());
    input.date_to = "2026-09-03".into();
    assert!(build(&input, None).is_err());
    Ok(())
}

#[test]
fn extended_years_cannot_overflow_padded_date_arithmetic() {
    let mut input = input();
    input.buffer_minutes = 120;
    for date in ["+262142-12-31", "-262143-01-01", "2026-8-03"] {
        input.date_from = date.into();
        input.date_to = date.into();
        assert!(build(&input, None).is_err());
    }
}

#[test]
fn optional_unknown_is_explicit_and_ranks_after_equal_known_conflict() -> Result<()> {
    let input = input();
    let plan = build(&input, None)?;
    let mut pages = free_pages(&input, &plan);
    *pages
        .first_mut()
        .ok_or_else(missing)?
        .participants
        .as_mut()
        .ok_or_else(missing)?
        .get_mut(1)
        .ok_or_else(missing)? = exact(
        input.participants.get(1).ok_or_else(missing)?,
        vec![
            FreeBusyStatus::NoData,
            FreeBusyStatus::NoData,
            FreeBusyStatus::Busy,
            FreeBusyStatus::Busy,
        ],
    );
    let records = prepare(&input.participants, pages)?;
    let output = find("work".into(), &plan, &records, &input)?;
    assert!(output.windows.is_empty());
    assert_eq!(
        output.suggestions.first().ok_or_else(missing)?.starts_at,
        "2026-08-03T10:00:00+00:00"
    );
    assert!(output.suggestions.iter().all(evaluate::required_safe));
    assert!(output.suggestions.iter().any(|suggestion| suggestion.conflicts.iter().any(
        |conflict| {
            conflict.role == CalendarParticipantRole::Optional
                && conflict.reasons.contains(&CalendarSlotConflictReason::Unknown)
        }
    )));
    Ok(())
}
