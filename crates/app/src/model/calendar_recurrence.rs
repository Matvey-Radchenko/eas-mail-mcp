use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ScheduleWeekday;

/// Explicit mutation boundary for a recurring event.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum CalendarScope {
    /// Entire series, including its exceptions.
    Series,
    /// Only the original occurrence selected by event_ref.
    Occurrence,
    /// The selected occurrence and every later one.
    Following,
}

/// Supported Gregorian repeat frequencies.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum CalendarFrequency {
    /// Every N days.
    Daily,
    /// Selected weekdays every N weeks.
    Weekly,
    /// Date or ordinal weekday every N months.
    Monthly,
    /// Date or ordinal weekday every N years.
    Yearly,
}

/// Explicit ending for a repeat rule.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum CalendarRecurrenceEnd {
    /// No ending date.
    Never,
    /// Count includes the first and deleted occurrences.
    Count {
        /// Total number of occurrences.
        #[schemars(range(min = 1, max = 65535))]
        count: u16,
    },
    /// Last local date that may contain an original occurrence start.
    Until {
        /// Inclusive date in YYYY-MM-DD format, in the event timezone.
        date: String,
    },
}

/// Repeat rule. Date selectors default to the initial event's local date.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarRecurrenceInput {
    /// Repeat frequency.
    pub frequency: CalendarFrequency,
    /// Positive interval, default one.
    #[serde(default = "one")]
    #[schemars(range(min = 1, max = 999))]
    pub interval: u16,
    /// Weekly days, or weekdays selected by week_of_month.
    #[serde(default)]
    pub weekdays: Vec<ScheduleWeekday>,
    /// Date selector for monthly or yearly repeats.
    pub day_of_month: Option<u8>,
    /// Relative selector: 1-4, or 5 for the last matching weekday.
    pub week_of_month: Option<u8>,
    /// Yearly month selector, 1-12.
    pub month: Option<u8>,
    /// Explicit termination; never-ending series must opt in.
    pub end: CalendarRecurrenceEnd,
}

const fn one() -> u16 {
    1
}
