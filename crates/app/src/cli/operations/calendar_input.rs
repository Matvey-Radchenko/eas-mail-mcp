use super::calendar_args::{
    AttendeeArgs, CalendarAgendaArgs, CalendarAvailabilityArgs, CalendarCancelArgs,
    CalendarCreateArgs, CalendarDeleteArgs, CalendarFindSlotsArgs, CalendarGetArgs,
    CalendarRespondArgs, CalendarSearchArgs, CalendarUpdateArgs, ScheduleArgs,
};
use super::common::{BusyStatusArg, ResponseArg};
use super::input::{
    body, comment, ensure_flag_mode, idempotency_key, invalid, optional_body, read_json,
    read_write_json, required, selected,
};
use crate::Result;
use crate::model::{
    CalendarAttendeeInput, CalendarAttendeeRole, CalendarAvailabilityInput, CalendarBusyStatus,
    CalendarCancelInput, CalendarCreateInput, CalendarDeleteInput, CalendarFindSlotsInput,
    CalendarGetInput, CalendarRespondInput, CalendarResponseChoice, CalendarScheduleInput,
    CalendarSearchInput, CalendarUpdateInput, ScheduleWeekday, WorkingHoursInput,
};

pub(super) fn availability(
    arguments: CalendarAvailabilityArgs,
) -> Result<CalendarAvailabilityInput> {
    let has_flags = availability_flags(&arguments);
    ensure_flag_mode(arguments.source.input.as_ref(), has_flags)?;
    arguments.source.input.map_or_else(
        || {
            Ok(CalendarAvailabilityInput {
                account_id: arguments.account,
                participants: arguments.participants,
                date_from: required(arguments.date_from, "from")?,
                date_to: required(arguments.date_to, "to")?,
                time_zone: required(arguments.time_zone, "time_zone")?,
                working_hours: working_hours(&arguments.working_hours)?,
            })
        },
        |path| read_json(&path),
    )
}

pub(super) fn find_slots(arguments: CalendarFindSlotsArgs) -> Result<CalendarFindSlotsInput> {
    let has_flags = availability_flags(&arguments.availability)
        || arguments.duration.is_some()
        || arguments.allow_tentative
        || arguments.limit.is_some();
    ensure_flag_mode(arguments.availability.source.input.as_ref(), has_flags)?;
    if let Some(path) = arguments.availability.source.input {
        return read_json(&path);
    }
    Ok(CalendarFindSlotsInput {
        account_id: arguments.availability.account,
        participants: arguments.availability.participants,
        date_from: required(arguments.availability.date_from, "from")?,
        date_to: required(arguments.availability.date_to, "to")?,
        time_zone: required(arguments.availability.time_zone, "time_zone")?,
        working_hours: working_hours(&arguments.availability.working_hours)?,
        duration_minutes: arguments.duration.ok_or_else(|| invalid("duration is required"))?,
        allow_tentative: arguments.allow_tentative,
        limit: arguments.limit,
    })
}

pub(super) fn search(arguments: CalendarSearchArgs) -> Result<CalendarSearchInput> {
    let has_flags =
        arguments.query.is_some() || !arguments.accounts.is_empty() || arguments.limit.is_some();
    ensure_flag_mode(arguments.source.input.as_ref(), has_flags)?;
    let input = arguments.source.input.map_or_else(
        || {
            Ok(CalendarSearchInput {
                query: Some(required(arguments.query, "query")?),
                date_from: None,
                date_to: None,
                time_zone: None,
                account_ids: selected(arguments.accounts),
                limit: arguments.limit,
            })
        },
        |path| read_json(&path),
    )?;
    require_search_mode(input)
}

pub(super) fn agenda(arguments: CalendarAgendaArgs) -> Result<CalendarSearchInput> {
    let has_flags = arguments.date_from.is_some()
        || arguments.date_to.is_some()
        || arguments.time_zone.is_some()
        || !arguments.accounts.is_empty()
        || arguments.limit.is_some();
    ensure_flag_mode(arguments.source.input.as_ref(), has_flags)?;
    let input = arguments.source.input.map_or_else(
        || {
            Ok(CalendarSearchInput {
                query: None,
                date_from: Some(required(arguments.date_from, "from")?),
                date_to: Some(required(arguments.date_to, "to")?),
                time_zone: Some(required(arguments.time_zone, "time_zone")?),
                account_ids: selected(arguments.accounts),
                limit: arguments.limit,
            })
        },
        |path| read_json(&path),
    )?;
    require_agenda_mode(input)
}

pub(super) fn get(arguments: CalendarGetArgs) -> Result<CalendarGetInput> {
    ensure_flag_mode(
        arguments.source.input.as_ref(),
        arguments.event_ref.is_some() || arguments.body_limit.is_some(),
    )?;
    arguments.source.input.map_or_else(
        || {
            Ok(CalendarGetInput {
                event_ref: required(arguments.event_ref, "event_ref")?,
                body_limit: arguments.body_limit,
            })
        },
        |path| read_json(&path),
    )
}

pub(super) fn create(arguments: CalendarCreateArgs) -> Result<(CalendarCreateInput, bool)> {
    let has_flags = arguments.account.is_some()
        || arguments.subject.is_some()
        || schedule_flags(&arguments.schedule)
        || arguments.recurrence.has_flags()
        || body_flags(&arguments.content)
        || arguments.location.is_some()
        || arguments.reminder.is_some()
        || arguments.busy_status.is_some()
        || attendee_flags(&arguments.attendees)
        || arguments.control.idempotency_key.is_some();
    ensure_flag_mode(arguments.source.input.as_ref(), has_flags)?;
    let input = if let Some(path) = arguments.source.input {
        read_write_json(&path, &arguments.control)?
    } else {
        CalendarCreateInput {
            recurrence: arguments.recurrence.into_input()?,
            account_id: required(arguments.account, "account")?,
            subject: required(arguments.subject, "subject")?,
            schedule: required_schedule(arguments.schedule)?,
            body: body(&arguments.content)?,
            location: arguments.location.unwrap_or_default(),
            reminder_minutes: arguments.reminder,
            busy_status: arguments
                .busy_status
                .map_or_else(CalendarBusyStatus::default, busy_status),
            attendees: attendees(arguments.attendees),
            idempotency_key: idempotency_key(&arguments.control),
        }
    };
    Ok((input, arguments.control.yes))
}

pub(super) fn update(arguments: CalendarUpdateArgs) -> Result<(CalendarUpdateInput, bool)> {
    let has_flags = arguments.scope.is_some()
        || arguments.event_ref.is_some()
        || arguments.subject.is_some()
        || schedule_flags(&arguments.schedule)
        || arguments.recurrence.has_flags()
        || body_flags(&arguments.content)
        || arguments.location.is_some()
        || arguments.reminder.is_some()
        || arguments.clear_reminder
        || arguments.busy_status.is_some()
        || attendee_flags(&arguments.attendees)
        || arguments.clear_attendees
        || arguments.control.idempotency_key.is_some();
    ensure_flag_mode(arguments.source.input.as_ref(), has_flags)?;
    let input = if let Some(path) = arguments.source.input {
        read_write_json(&path, &arguments.control)?
    } else {
        CalendarUpdateInput {
            scope: arguments.scope,
            recurrence: arguments.recurrence.into_input()?,
            event_ref: required(arguments.event_ref, "event_ref")?,
            subject: arguments.subject,
            schedule: optional_schedule(arguments.schedule)?,
            body: optional_body(&arguments.content)?,
            location: arguments.location,
            reminder_minutes: arguments.reminder,
            clear_reminder: arguments.clear_reminder,
            busy_status: arguments.busy_status.map(busy_status),
            attendees: replacement_attendees(arguments.attendees, arguments.clear_attendees),
            idempotency_key: idempotency_key(&arguments.control),
        }
    };
    Ok((input, arguments.control.yes))
}

pub(super) fn delete(arguments: CalendarDeleteArgs) -> Result<(CalendarDeleteInput, bool)> {
    ensure_flag_mode(
        arguments.source.input.as_ref(),
        arguments.scope.is_some()
            || arguments.event_ref.is_some()
            || arguments.control.idempotency_key.is_some(),
    )?;
    let input = if let Some(path) = arguments.source.input {
        read_write_json(&path, &arguments.control)?
    } else {
        CalendarDeleteInput {
            scope: arguments.scope,
            event_ref: required(arguments.event_ref, "event_ref")?,
            idempotency_key: idempotency_key(&arguments.control),
        }
    };
    Ok((input, arguments.control.yes))
}

pub(super) fn cancel(arguments: CalendarCancelArgs) -> Result<(CalendarCancelInput, bool)> {
    let has_flags = arguments.scope.is_some()
        || arguments.event_ref.is_some()
        || comment_flags(&arguments.content)
        || arguments.control.idempotency_key.is_some();
    ensure_flag_mode(arguments.source.input.as_ref(), has_flags)?;
    let input = if let Some(path) = arguments.source.input {
        read_write_json(&path, &arguments.control)?
    } else {
        CalendarCancelInput {
            scope: arguments.scope,
            event_ref: required(arguments.event_ref, "event_ref")?,
            comment: comment(&arguments.content)?,
            idempotency_key: idempotency_key(&arguments.control),
        }
    };
    Ok((input, arguments.control.yes))
}

pub(super) fn respond(arguments: CalendarRespondArgs) -> Result<(CalendarRespondInput, bool)> {
    let has_flags = arguments.scope.is_some()
        || arguments.event_ref.is_some()
        || arguments.response.is_some()
        || comment_flags(&arguments.content)
        || arguments.control.idempotency_key.is_some();
    ensure_flag_mode(arguments.source.input.as_ref(), has_flags)?;
    let input = if let Some(path) = arguments.source.input {
        read_write_json(&path, &arguments.control)?
    } else {
        CalendarRespondInput {
            scope: arguments.scope,
            event_ref: required(arguments.event_ref, "event_ref")?,
            response: response(arguments.response.ok_or_else(|| invalid("response is required"))?),
            comment: comment(&arguments.content)?,
            idempotency_key: idempotency_key(&arguments.control),
        }
    };
    Ok((input, arguments.control.yes))
}

fn working_hours(values: &[String]) -> Result<Vec<WorkingHoursInput>> {
    if values.is_empty() {
        return Err(invalid("at least one working-hours interval is required"));
    }
    values.iter().map(|value| parse_working_hours(value)).collect()
}

fn parse_working_hours(value: &str) -> Result<WorkingHoursInput> {
    let (days, times) =
        value.split_once('@').ok_or_else(|| invalid("working-hours must use days@HH:MM-HH:MM"))?;
    let (start, end) =
        times.split_once('-').ok_or_else(|| invalid("working-hours must use days@HH:MM-HH:MM"))?;
    let weekdays = days.split(',').map(parse_weekday).collect::<Result<Vec<_>>>()?;
    if weekdays.is_empty() || start.is_empty() || end.is_empty() {
        return Err(invalid("working-hours must use days@HH:MM-HH:MM"));
    }
    Ok(WorkingHoursInput { weekdays, start: start.to_owned(), end: end.to_owned() })
}

pub(super) fn parse_weekday(value: &str) -> Result<ScheduleWeekday> {
    match value.trim().to_ascii_lowercase().as_str() {
        "mon" => Ok(ScheduleWeekday::Mon),
        "tue" => Ok(ScheduleWeekday::Tue),
        "wed" => Ok(ScheduleWeekday::Wed),
        "thu" => Ok(ScheduleWeekday::Thu),
        "fri" => Ok(ScheduleWeekday::Fri),
        "sat" => Ok(ScheduleWeekday::Sat),
        "sun" => Ok(ScheduleWeekday::Sun),
        _ => Err(invalid("working-hours contains an unknown weekday")),
    }
}

fn required_schedule(arguments: ScheduleArgs) -> Result<CalendarScheduleInput> {
    optional_schedule(arguments)?.ok_or_else(|| invalid("event schedule is required"))
}

fn optional_schedule(arguments: ScheduleArgs) -> Result<Option<CalendarScheduleInput>> {
    match (
        arguments.start,
        arguments.end,
        arguments.all_day_start,
        arguments.all_day_end,
        arguments.time_zone,
    ) {
        (None, None, None, None, None) => Ok(None),
        (Some(start), Some(end), None, None, Some(time_zone)) => {
            Ok(Some(CalendarScheduleInput::Timed { start, end, time_zone }))
        }
        (None, None, Some(start_date), Some(end_date), Some(time_zone)) => {
            Ok(Some(CalendarScheduleInput::AllDay { start_date, end_date, time_zone }))
        }
        _ => Err(invalid(
            "use --start/--end/--time-zone or --all-day-start/--all-day-end/--time-zone",
        )),
    }
}

fn attendees(arguments: AttendeeArgs) -> Vec<CalendarAttendeeInput> {
    let mut values = attendee_group(arguments.required, CalendarAttendeeRole::Required);
    values.extend(attendee_group(arguments.optional, CalendarAttendeeRole::Optional));
    values.extend(attendee_group(arguments.resource, CalendarAttendeeRole::Resource));
    values
}

fn attendee_group(values: Vec<String>, role: CalendarAttendeeRole) -> Vec<CalendarAttendeeInput> {
    values.into_iter().map(|email| CalendarAttendeeInput { email, name: None, role }).collect()
}

fn replacement_attendees(
    arguments: AttendeeArgs,
    clear: bool,
) -> Option<Vec<CalendarAttendeeInput>> {
    if clear {
        Some(Vec::new())
    } else if attendee_flags(&arguments) {
        Some(attendees(arguments))
    } else {
        None
    }
}

fn require_search_mode(input: CalendarSearchInput) -> Result<CalendarSearchInput> {
    let valid_query = input.query.as_deref().is_some_and(|value| !value.trim().is_empty());
    if valid_query
        && input.date_from.is_none()
        && input.date_to.is_none()
        && input.time_zone.is_none()
    {
        Ok(input)
    } else {
        Err(invalid("calendar search accepts a query and no date range"))
    }
}

fn require_agenda_mode(input: CalendarSearchInput) -> Result<CalendarSearchInput> {
    if input.query.is_none()
        && input.date_from.is_some()
        && input.date_to.is_some()
        && input.time_zone.is_some()
    {
        Ok(input)
    } else {
        Err(invalid("calendar agenda requires from, to, and time_zone without query"))
    }
}

fn availability_flags(arguments: &CalendarAvailabilityArgs) -> bool {
    arguments.account.is_some()
        || !arguments.participants.is_empty()
        || arguments.date_from.is_some()
        || arguments.date_to.is_some()
        || arguments.time_zone.is_some()
        || !arguments.working_hours.is_empty()
}

fn schedule_flags(arguments: &ScheduleArgs) -> bool {
    arguments.start.is_some()
        || arguments.end.is_some()
        || arguments.all_day_start.is_some()
        || arguments.all_day_end.is_some()
        || arguments.time_zone.is_some()
}

fn attendee_flags(arguments: &AttendeeArgs) -> bool {
    !arguments.required.is_empty()
        || !arguments.optional.is_empty()
        || !arguments.resource.is_empty()
}

fn body_flags(source: &super::common::BodySource) -> bool {
    source.body.is_some() || source.body_file.is_some() || source.body_stdin
}

fn comment_flags(source: &super::common::CommentSource) -> bool {
    source.comment.is_some() || source.comment_file.is_some() || source.comment_stdin
}

const fn busy_status(value: BusyStatusArg) -> CalendarBusyStatus {
    match value {
        BusyStatusArg::Free => CalendarBusyStatus::Free,
        BusyStatusArg::Tentative => CalendarBusyStatus::Tentative,
        BusyStatusArg::Busy => CalendarBusyStatus::Busy,
        BusyStatusArg::OutOfOffice => CalendarBusyStatus::OutOfOffice,
    }
}

const fn response(value: ResponseArg) -> CalendarResponseChoice {
    match value {
        ResponseArg::Accept => CalendarResponseChoice::Accept,
        ResponseArg::Tentative => CalendarResponseChoice::Tentative,
        ResponseArg::Decline => CalendarResponseChoice::Decline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_hours_shorthand_is_strict() -> anyhow::Result<()> {
        let parsed = parse_working_hours("mon,tue,fri@09:00-18:00")?;
        assert_eq!(
            parsed.weekdays,
            vec![ScheduleWeekday::Mon, ScheduleWeekday::Tue, ScheduleWeekday::Fri]
        );
        assert_eq!(parsed.start, "09:00");
        assert_eq!(parsed.end, "18:00");
        for invalid in ["", "mon", "mon@09:00", "monday@09:00-18:00"] {
            assert!(parse_working_hours(invalid).is_err());
        }
        Ok(())
    }

    #[test]
    fn schedule_flags_require_one_complete_shape() -> anyhow::Result<()> {
        let timed = optional_schedule(ScheduleArgs {
            start: Some("2026-01-02T10:00:00Z".into()),
            end: Some("2026-01-02T11:00:00Z".into()),
            time_zone: Some("UTC".into()),
            ..ScheduleArgs::default()
        })?;
        assert!(matches!(timed, Some(CalendarScheduleInput::Timed { .. })));
        assert!(optional_schedule(ScheduleArgs::default())?.is_none());
        assert!(
            optional_schedule(ScheduleArgs {
                start: Some("2026-01-02T10:00:00Z".into()),
                time_zone: Some("UTC".into()),
                ..ScheduleArgs::default()
            })
            .is_err()
        );
        Ok(())
    }
}
