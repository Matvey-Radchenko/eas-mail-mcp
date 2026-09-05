use eas_mail_protocol::{
    CandidateAvailability, FreeBusyStatus, RecipientAvailability, RecipientResolution,
    ResolvedRecipient,
};

use super::*;
use crate::model::{
    CalendarFindRecurringSlotsInput, CalendarParticipantOptions, CalendarSlotConflictReason,
    ScheduleWeekday, WorkingHoursInput,
};

pub(super) fn input() -> CalendarFindSlotsInput {
    CalendarFindSlotsInput {
        account_id: Some("work".into()),
        participants: vec!["required@example.invalid".into(), "optional@example.invalid".into()],
        date_from: "2026-08-03".into(),
        date_to: "2026-08-03".into(),
        time_zone: "UTC".into(),
        working_hours: vec![WorkingHoursInput {
            weekdays: vec![ScheduleWeekday::Mon],
            start: "09:00".into(),
            end: "11:00".into(),
        }],
        duration_minutes: 30,
        allow_tentative: false,
        limit: None,
        buffer_minutes: 0,
        participant_options: vec![CalendarParticipantOptions {
            input: "optional@example.invalid".into(),
            role: CalendarParticipantRole::Optional,
            time_zone: None,
            working_hours: None,
        }],
    }
}

pub(super) fn exact(input: &str, slots: Vec<FreeBusyStatus>) -> RecipientAvailability {
    RecipientAvailability {
        input: input.into(),
        resolution: RecipientResolution::Resolved,
        total_candidates: 1,
        candidates: vec![ResolvedRecipient {
            recipient_type: 1,
            display_name: "Person".into(),
            email: input.into(),
            availability: CandidateAvailability::Slots(slots),
        }],
    }
}

pub(super) fn free_pages(input: &CalendarFindSlotsInput, plan: &RankedPlan) -> Vec<Page> {
    plan.queries
        .iter()
        .map(|range| Page {
            range: *range,
            participants: Some(
                input
                    .participants
                    .iter()
                    .map(|person| {
                        let count = ((range.end - range.start).num_seconds() + 1799) / 1800;
                        exact(person, (0..count).map(|_| FreeBusyStatus::Free).collect())
                    })
                    .collect(),
            ),
        })
        .collect()
}

#[test]
fn optional_conflicts_are_ranked_and_old_windows_still_require_everyone() -> Result<()> {
    let input = input();
    let plan = build(&input, None)?;
    let mut pages = free_pages(&input, &plan);
    *pages
        .first_mut()
        .ok_or_else(super::super::state_error)?
        .participants
        .as_mut()
        .ok_or_else(super::super::state_error)?
        .get_mut(1)
        .ok_or_else(super::super::state_error)? = exact(
        input.participants.get(1).ok_or_else(super::super::state_error)?,
        vec![
            FreeBusyStatus::Busy,
            FreeBusyStatus::Busy,
            FreeBusyStatus::Free,
            FreeBusyStatus::Free,
        ],
    );
    let records = prepare(&input.participants, pages)?;
    let data = find("work".into(), &plan, &records, &input)?;
    assert_eq!(data.precision_minutes, 30);
    assert_eq!(
        data.suggestions.first().ok_or_else(super::super::state_error)?.starts_at,
        "2026-08-03T10:00:00+00:00"
    );
    assert_eq!(
        data.windows.first().ok_or_else(super::super::state_error)?.window_start,
        "2026-08-03T10:00:00+00:00"
    );
    assert!(data.suggestions.iter().any(|value| !value.conflicts.is_empty()));
    assert!(data.suggestions.iter().all(evaluate::required_safe));
    Ok(())
}

#[test]
fn buffer_queries_outside_working_hours_and_unknown_is_never_free() -> Result<()> {
    let mut input = input();
    input.buffer_minutes = 15;
    let plan = build(&input, None)?;
    assert_eq!(
        plan.queries.first().ok_or_else(super::super::state_error)?.start.to_rfc3339(),
        "2026-08-03T08:45:00+00:00"
    );
    let mut pages = free_pages(&input, &plan);
    *pages
        .first_mut()
        .ok_or_else(super::super::state_error)?
        .participants
        .as_mut()
        .ok_or_else(super::super::state_error)?
        .first_mut()
        .ok_or_else(super::super::state_error)? = exact(
        input.participants.first().ok_or_else(super::super::state_error)?,
        vec![
            FreeBusyStatus::Busy,
            FreeBusyStatus::Free,
            FreeBusyStatus::Free,
            FreeBusyStatus::NoData,
            FreeBusyStatus::Free,
        ],
    );
    let records = prepare(&input.participants, pages)?;
    let data = find("work".into(), &plan, &records, &input)?;
    assert_eq!(data.suggestions.len(), 1);
    assert_eq!(
        data.suggestions.first().ok_or_else(super::super::state_error)?.starts_at,
        "2026-08-03T09:30:00+00:00"
    );
    assert!(data.participants.first().ok_or_else(super::super::state_error)?.has_no_data);
    Ok(())
}

#[test]
fn personal_timezone_and_working_hours_control_required_participant() -> Result<()> {
    let mut input = input();
    input.participant_options.push(CalendarParticipantOptions {
        input: input.participants.first().ok_or_else(super::super::state_error)?.clone(),
        role: CalendarParticipantRole::Required,
        time_zone: Some("Europe/Belgrade".into()),
        working_hours: None,
    });
    let plan = build(&input, None)?;
    let records = prepare(&input.participants, free_pages(&input, &plan))?;
    assert!(find("work".into(), &plan, &records, &input)?.suggestions.is_empty());
    input.participant_options.get_mut(1).ok_or_else(super::super::state_error)?.working_hours =
        Some(vec![WorkingHoursInput {
            weekdays: vec![ScheduleWeekday::Mon],
            start: "12:00".into(),
            end: "13:00".into(),
        }]);
    let plan = build(&input, None)?;
    let records = prepare(&input.participants, free_pages(&input, &plan))?;
    assert_eq!(
        find("work".into(), &plan, &records, &input)?
            .suggestions
            .first()
            .ok_or_else(super::super::state_error)?
            .starts_at,
        "2026-08-03T10:00:00+00:00"
    );
    Ok(())
}

#[test]
fn weekly_preserves_wall_clock_across_dst_and_marks_failed_week_unknown() -> Result<()> {
    let mut schedule = input();
    schedule.date_from = "2026-10-19".into();
    schedule.date_to = "2026-11-02".into();
    schedule.time_zone = "Europe/Belgrade".into();
    schedule.limit = Some(1);
    let input = CalendarFindRecurringSlotsInput { schedule, weekday: ScheduleWeekday::Mon };
    let plan = build(&input.schedule, Some(input.weekday))?;
    assert_eq!(plan.queries.len(), 3);
    let mut pages = free_pages(&input.schedule, &plan);
    pages.get_mut(1).ok_or_else(super::super::state_error)?.participants = None;
    let records = prepare(&input.schedule.participants, pages)?;
    let data = find_recurring("work".into(), &plan, &records, &input)?;
    let pattern = &data.suggestions.first().ok_or_else(super::super::state_error)?;
    assert_eq!(pattern.required_available_occurrences, 2);
    assert_eq!(
        pattern.occurrences.first().ok_or_else(super::super::state_error)?.starts_at,
        "2026-10-19T09:00:00+02:00"
    );
    assert_eq!(
        pattern.occurrences.get(1).ok_or_else(super::super::state_error)?.starts_at,
        "2026-10-26T09:00:00+01:00"
    );
    assert_eq!(
        pattern.occurrences.get(2).ok_or_else(super::super::state_error)?.starts_at,
        "2026-11-02T09:00:00+01:00"
    );
    assert!(
        pattern
            .occurrences
            .get(1)
            .ok_or_else(super::super::state_error)?
            .conflicts
            .first()
            .ok_or_else(super::super::state_error)?
            .reasons
            .contains(&CalendarSlotConflictReason::Unknown)
    );
    Ok(())
}

#[test]
fn recurring_caps_range_queries_and_rejects_all_optional() -> Result<()> {
    let mut input = input();
    input.date_to = "2026-10-31".into();
    assert_eq!(build(&input, Some(ScheduleWeekday::Mon))?.queries.len(), 13);
    assert!(build(&input, None).is_err());
    input.date_to = "2026-11-01".into();
    assert!(build(&input, Some(ScheduleWeekday::Mon)).is_err());
    input.date_to = input.date_from.clone();
    input.participant_options.push(CalendarParticipantOptions {
        input: input.participants.first().ok_or_else(super::super::state_error)?.clone(),
        role: CalendarParticipantRole::Optional,
        time_zone: None,
        working_hours: None,
    });
    assert!(build(&input, None).is_err());
    Ok(())
}

#[test]
fn flat_recurring_input_accepts_weekday_and_rejects_unknown_fields() -> Result<()> {
    let json = serde_json::json!({"participants":["a"],"date_from":"2026-08-03","date_to":"2026-08-10","time_zone":"UTC","working_hours":[{"weekdays":["mon"],"start":"09:00","end":"10:00"}],"duration_minutes":30,"weekday":"mon"});
    assert!(serde_json::from_value::<CalendarFindRecurringSlotsInput>(json.clone()).is_ok());
    let mut typo = json;
    typo.as_object_mut()
        .ok_or_else(super::super::state_error)?
        .insert("invalid".into(), true.into());
    assert!(serde_json::from_value::<CalendarFindRecurringSlotsInput>(typo).is_err());
    Ok(())
}

#[test]
fn required_tentative_and_unresolved_participants_are_never_silently_free() -> Result<()> {
    let mut input = input();
    let plan = build(&input, None)?;
    let mut pages = free_pages(&input, &plan);
    *pages
        .first_mut()
        .ok_or_else(super::super::state_error)?
        .participants
        .as_mut()
        .ok_or_else(super::super::state_error)?
        .first_mut()
        .ok_or_else(super::super::state_error)? = exact(
        input.participants.first().ok_or_else(super::super::state_error)?,
        vec![FreeBusyStatus::Tentative; 4],
    );
    let records = prepare(&input.participants, pages)?;
    assert!(find("work".into(), &plan, &records, &input)?.suggestions.is_empty());
    input.allow_tentative = true;
    let data = find("work".into(), &plan, &records, &input)?;
    assert_eq!(
        data.suggestions.first().ok_or_else(super::super::state_error)?.tentative_participants,
        vec![input.participants.first().ok_or_else(super::super::state_error)?.clone()]
    );
    let mut pages = free_pages(&input, &plan);
    *pages
        .first_mut()
        .ok_or_else(super::super::state_error)?
        .participants
        .as_mut()
        .ok_or_else(super::super::state_error)?
        .first_mut()
        .ok_or_else(super::super::state_error)? = RecipientAvailability {
        input: input.participants.first().ok_or_else(super::super::state_error)?.clone(),
        resolution: RecipientResolution::NotFound,
        total_candidates: 0,
        candidates: Vec::new(),
    };
    let records = prepare(&input.participants, pages)?;
    assert!(find("work".into(), &plan, &records, &input)?.suggestions.is_empty());
    input.duration_minutes = 17;
    assert!(build(&input, None).is_err());
    Ok(())
}

#[test]
fn recurring_ranks_required_conflicts_before_optional_ones_and_unknown_last() -> Result<()> {
    let mut schedule = input();
    schedule.date_to = "2026-08-10".into();
    let input = CalendarFindRecurringSlotsInput { schedule, weekday: ScheduleWeekday::Mon };
    let plan = build(&input.schedule, Some(input.weekday))?;
    let mut pages = free_pages(&input.schedule, &plan);
    for page in &mut pages {
        let participants = page.participants.as_mut().ok_or_else(super::super::state_error)?;
        *participants.first_mut().ok_or_else(super::super::state_error)? = exact(
            input.schedule.participants.first().ok_or_else(super::super::state_error)?,
            vec![
                FreeBusyStatus::Busy,
                FreeBusyStatus::NoData,
                FreeBusyStatus::Free,
                FreeBusyStatus::Free,
            ],
        );
        *participants.get_mut(1).ok_or_else(super::super::state_error)? = exact(
            input.schedule.participants.get(1).ok_or_else(super::super::state_error)?,
            vec![
                FreeBusyStatus::Free,
                FreeBusyStatus::Free,
                FreeBusyStatus::Busy,
                FreeBusyStatus::Busy,
            ],
        );
    }
    let records = prepare(&input.schedule.participants, pages)?;
    let data = find_recurring("work".into(), &plan, &records, &input)?;
    assert_eq!(
        data.suggestions.first().ok_or_else(super::super::state_error)?.local_start_time,
        "10:00"
    );
    assert_eq!(
        data.suggestions
            .first()
            .ok_or_else(super::super::state_error)?
            .required_available_occurrences,
        2
    );
    assert_eq!(
        data.suggestions.get(3).ok_or_else(super::super::state_error)?.local_start_time,
        "09:00"
    );
    assert_eq!(
        data.suggestions
            .get(3)
            .ok_or_else(super::super::state_error)?
            .required_available_occurrences,
        0
    );
    assert!(
        data.suggestions
            .get(3)
            .ok_or_else(super::super::state_error)?
            .occurrences
            .first()
            .ok_or_else(super::super::state_error)?
            .conflicts
            .first()
            .ok_or_else(super::super::state_error)?
            .reasons
            .contains(&CalendarSlotConflictReason::Busy)
    );
    Ok(())
}

#[test]
fn dst_gap_or_fold_excludes_the_whole_weekly_wall_clock_pattern() -> Result<()> {
    for (from, to) in [("2026-03-22", "2026-03-29"), ("2026-10-18", "2026-10-25")] {
        let mut schedule = input();
        schedule.date_from = from.into();
        schedule.date_to = to.into();
        schedule.time_zone = "Europe/Belgrade".into();
        schedule.working_hours = vec![WorkingHoursInput {
            weekdays: vec![ScheduleWeekday::Sun],
            start: "01:00".into(),
            end: "04:00".into(),
        }];
        let input = CalendarFindRecurringSlotsInput { schedule, weekday: ScheduleWeekday::Sun };
        let plan = build(&input.schedule, Some(input.weekday))?;
        let records = prepare(&input.schedule.participants, free_pages(&input.schedule, &plan))?;
        let data = find_recurring("work".into(), &plan, &records, &input)?;
        assert!(
            data.suggestions.iter().all(|pattern| !pattern.local_start_time.starts_with("02:"))
        );
    }
    Ok(())
}
