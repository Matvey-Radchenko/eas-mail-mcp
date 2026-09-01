use chrono::{Datelike as _, NaiveDate, Timelike as _};
use eas_mail_protocol::{
    CalendarApplication, CalendarRecurrence, RecurrenceEnd, RecurrencePattern,
};

use super::{invalid, validate_member, zone};
use crate::Result;
use crate::model::{
    CalendarFrequency, CalendarRecurrenceEnd, CalendarRecurrenceInput, ScheduleWeekday,
};

pub(in crate::runtime) fn prepare(
    input: &CalendarRecurrenceInput,
    event: &CalendarApplication,
) -> Result<CalendarRecurrence> {
    let zone = zone(event)?;
    let local = zone.to_local(event.starts_at)?;
    if local.nanosecond() != 0 || event.ends_at.nanosecond() != 0 {
        return Err(invalid("recurring start and end must be whole seconds"));
    }
    let day = input
        .day_of_month
        .unwrap_or(u8::try_from(local.day()).map_err(|_| invalid("invalid day"))?);
    let month =
        input.month.unwrap_or(u8::try_from(local.month()).map_err(|_| invalid("invalid month"))?);
    let mut mask = 0_u8;
    for value in &input.weekdays {
        let bit = 1 << weekday(*value);
        if mask & bit != 0 {
            return Err(invalid("recurrence weekdays must be unique"));
        }
        mask |= bit;
    }
    validate_selectors(input)?;
    let days = if mask == 0 { 1 << local.weekday().num_days_from_sunday() } else { mask };
    let pattern = match (input.frequency, input.week_of_month) {
        (CalendarFrequency::Daily, _) => RecurrencePattern::Daily,
        (CalendarFrequency::Weekly, _) => RecurrencePattern::Weekly { days },
        (CalendarFrequency::Monthly, None) => RecurrencePattern::Monthly { day },
        (CalendarFrequency::Monthly, Some(week)) => {
            RecurrencePattern::MonthlyRelative { days, week }
        }
        (CalendarFrequency::Yearly, None) => RecurrencePattern::Yearly { month, day },
        (CalendarFrequency::Yearly, Some(week)) => {
            RecurrencePattern::YearlyRelative { month, days, week }
        }
    };
    let end = match &input.end {
        CalendarRecurrenceEnd::Never => RecurrenceEnd::Never,
        CalendarRecurrenceEnd::Count { count } => RecurrenceEnd::Count(*count),
        CalendarRecurrenceEnd::Until { date } => {
            let date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .map_err(|_| invalid("recurrence until must use YYYY-MM-DD"))?;
            let end = date
                .and_hms_opt(23, 59, 59)
                .ok_or_else(|| invalid("recurrence until is invalid"))?;
            RecurrenceEnd::Until(zone.to_utc(end)?)
        }
    };
    let rule = CalendarRecurrence { pattern, interval: input.interval, first_day_of_week: 1, end };
    rule.validate().map_err(|_| invalid("invalid recurrence rule"))?;
    let mut candidate = event.clone();
    candidate.properties.recurrence = Some(rule.clone());
    if validate_member(&candidate, event.starts_at)? != 1 {
        return Err(invalid("event start must be the first occurrence of its recurrence rule"));
    }
    Ok(rule)
}

fn validate_selectors(input: &CalendarRecurrenceInput) -> Result<()> {
    let date = input.day_of_month.is_some();
    let week = input.week_of_month.is_some();
    let days = !input.weekdays.is_empty();
    let month = input.month.is_some();
    let valid = match input.frequency {
        CalendarFrequency::Daily => !date && !week && !days && !month,
        CalendarFrequency::Weekly => !date && !week && !month,
        CalendarFrequency::Monthly => !(month || date && week || !week && days),
        CalendarFrequency::Yearly => !(date && week) && (!days || week),
    };
    if valid { Ok(()) } else { Err(invalid("recurrence selectors do not match the frequency")) }
}

const fn weekday(value: ScheduleWeekday) -> u8 {
    match value {
        ScheduleWeekday::Sun => 0,
        ScheduleWeekday::Mon => 1,
        ScheduleWeekday::Tue => 2,
        ScheduleWeekday::Wed => 3,
        ScheduleWeekday::Thu => 4,
        ScheduleWeekday::Fri => 5,
        ScheduleWeekday::Sat => 6,
    }
}
