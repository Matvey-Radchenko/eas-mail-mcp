use clap::Args;

use super::input::invalid;
use crate::Result;
use crate::model::{CalendarFrequency, CalendarRecurrenceEnd, CalendarRecurrenceInput};

#[derive(Debug, Args, Default)]
pub(super) struct RecurrenceArgs {
    /// Repeat frequency; requires an explicit repeat ending.
    #[arg(long = "repeat", value_enum)]
    frequency: Option<CalendarFrequency>,
    /// Repeat every N days, weeks, months, or years.
    #[arg(long = "repeat-interval")]
    interval: Option<u16>,
    /// Weekday such as mon; repeat this flag for multiple days.
    #[arg(long = "repeat-weekday")]
    weekdays: Vec<String>,
    /// Day of month for monthly or yearly recurrence.
    #[arg(long = "repeat-day")]
    day: Option<u8>,
    /// Relative weekday ordinal: 1-4 or 5 for last.
    #[arg(long = "repeat-week")]
    week: Option<u8>,
    /// Month for yearly recurrence, 1-12.
    #[arg(long = "repeat-month")]
    month: Option<u8>,
    /// Inclusive local ending date, YYYY-MM-DD.
    #[arg(long = "repeat-until", conflicts_with_all = ["count", "forever"])]
    until: Option<String>,
    /// Total occurrences, including the first one.
    #[arg(long = "repeat-count", conflicts_with_all = ["until", "forever"])]
    count: Option<u16>,
    /// Explicitly create a series without an end.
    #[arg(long = "repeat-forever", conflicts_with_all = ["until", "count"])]
    forever: bool,
}

impl RecurrenceArgs {
    pub(super) fn has_flags(&self) -> bool {
        self.frequency.is_some()
            || self.interval.is_some()
            || !self.weekdays.is_empty()
            || self.day.is_some()
            || self.week.is_some()
            || self.month.is_some()
            || self.until.is_some()
            || self.count.is_some()
            || self.forever
    }

    pub(super) fn into_input(self) -> Result<Option<CalendarRecurrenceInput>> {
        if !self.has_flags() {
            return Ok(None);
        }
        let frequency =
            self.frequency.ok_or_else(|| invalid("recurrence options require --repeat"))?;
        let end = match (self.until, self.count, self.forever) {
            (Some(date), None, false) => CalendarRecurrenceEnd::Until { date },
            (None, Some(count), false) => CalendarRecurrenceEnd::Count { count },
            (None, None, true) => CalendarRecurrenceEnd::Never,
            _ => return Err(invalid("choose --repeat-until, --repeat-count, or --repeat-forever")),
        };
        let weekdays = self
            .weekdays
            .iter()
            .map(|value| super::calendar_input::parse_weekday(value))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(CalendarRecurrenceInput {
            frequency,
            interval: self.interval.unwrap_or(1),
            weekdays,
            day_of_month: self.day,
            week_of_month: self.week,
            month: self.month,
            end,
        }))
    }
}
