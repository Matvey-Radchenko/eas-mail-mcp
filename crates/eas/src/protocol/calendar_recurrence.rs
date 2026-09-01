use chrono::{Datelike as _, Timelike as _};
use std::collections::BTreeMap;

use crate::{CalendarRecurrence, EasError, RecurrenceEnd, RecurrencePattern, Result};

use super::tree::parse_datetime;

impl CalendarRecurrence {
    /// Parses the legacy public Calendar recurrence map without losing fields.
    pub fn from_fields(fields: &BTreeMap<String, String>) -> Result<Self> {
        const KNOWN: &[&str] = &[
            "type",
            "interval",
            "dayofweek",
            "dayofmonth",
            "weekofmonth",
            "monthofyear",
            "occurrences",
            "until",
            "firstdayofweek",
            "calendartype",
            "isleapmonth",
        ];
        if fields.keys().any(|key| !KNOWN.contains(&key.as_str()))
            || !matches!(number(fields, "calendartype", 1)?, 0 | 1)
            || number(fields, "isleapmonth", 0)? != 0
        {
            return Err(invalid());
        }
        let day = || small(fields, "dayofmonth");
        let days = || small(fields, "dayofweek");
        let week = || small(fields, "weekofmonth");
        let month = || small(fields, "monthofyear");
        let pattern = match number(fields, "type", 255)? {
            0 if !fields.contains_key("dayofweek") => RecurrencePattern::Daily,
            0 | 1 => RecurrencePattern::Weekly { days: days()? },
            2 => RecurrencePattern::Monthly { day: day()? },
            3 => RecurrencePattern::MonthlyRelative { days: days()?, week: week()? },
            5 => RecurrencePattern::Yearly { month: month()?, day: day()? },
            6 => {
                RecurrencePattern::YearlyRelative { month: month()?, days: days()?, week: week()? }
            }
            _ => return Err(invalid()),
        };
        let end = match (fields.get("occurrences"), fields.get("until")) {
            (None, None) => RecurrenceEnd::Never,
            (Some(count), None) => RecurrenceEnd::Count(count.parse().map_err(|_| invalid())?),
            (None, Some(until)) => {
                RecurrenceEnd::Until(parse_datetime(Some(until.clone())).ok_or_else(invalid)?)
            }
            _ => return Err(invalid()),
        };
        let value = Self {
            pattern,
            end,
            interval: number(fields, "interval", 1)?,
            first_day_of_week: u8::try_from(number(fields, "firstdayofweek", 0)?)
                .map_err(|_| invalid())?,
        };
        value.validate()?;
        // A recognized but inapplicable selector must not silently disappear.
        let rendered = value.to_fields();
        for key in ["dayofweek", "dayofmonth", "weekofmonth", "monthofyear"] {
            if fields.contains_key(key) && fields.get(key) != rendered.get(key) {
                return Err(invalid());
            }
        }
        Ok(value)
    }

    /// Validates field combinations before producing WBXML or invitations.
    pub fn validate(&self) -> Result<()> {
        let day = |value| (1..=31).contains(&value);
        let days = |value| (1..=127).contains(&value);
        let week = |value| (1..=5).contains(&value);
        let month = |value| (1..=12).contains(&value);
        let valid = match self.pattern {
            RecurrencePattern::Daily => true,
            RecurrencePattern::Weekly { days: value } => days(value),
            RecurrencePattern::Monthly { day: value } => day(value),
            RecurrencePattern::MonthlyRelative { days: mask, week: ordinal } => {
                days(mask) && week(ordinal)
            }
            RecurrencePattern::Yearly { month: m, day: d } => month(m) && day(d),
            RecurrencePattern::YearlyRelative { month: m, days: mask, week: ordinal } => {
                month(m) && days(mask) && week(ordinal)
            }
        };
        if !valid
            || !(1..=999).contains(&self.interval)
            || self.first_day_of_week > 6
            || matches!(self.end, RecurrenceEnd::Count(0))
            || matches!(self.end, RecurrenceEnd::Until(value) if !(1..=9999).contains(&value.year()) || value.nanosecond() != 0)
        {
            return Err(invalid());
        }
        Ok(())
    }

    /// Produces the stable legacy map used by read responses and recurrence expansion.
    pub fn to_fields(&self) -> BTreeMap<String, String> {
        let mut values = BTreeMap::new();
        let (kind, day, days, week, month) = match self.pattern {
            RecurrencePattern::Daily => (0, None, None, None, None),
            RecurrencePattern::Weekly { days } => (1, None, Some(days), None, None),
            RecurrencePattern::Monthly { day } => (2, Some(day), None, None, None),
            RecurrencePattern::MonthlyRelative { days, week } => {
                (3, None, Some(days), Some(week), None)
            }
            RecurrencePattern::Yearly { month, day } => (5, Some(day), None, None, Some(month)),
            RecurrencePattern::YearlyRelative { month, days, week } => {
                (6, None, Some(days), Some(week), Some(month))
            }
        };
        values.insert("type".into(), kind.to_string());
        values.insert("interval".into(), self.interval.to_string());
        values.insert("firstdayofweek".into(), self.first_day_of_week.to_string());
        for (key, value) in [
            ("dayofmonth", day),
            ("dayofweek", days),
            ("weekofmonth", week),
            ("monthofyear", month),
        ] {
            if let Some(value) = value {
                values.insert(key.into(), value.to_string());
            }
        }
        match self.end {
            RecurrenceEnd::Never => {}
            RecurrenceEnd::Count(value) => {
                values.insert("occurrences".into(), value.to_string());
            }
            RecurrenceEnd::Until(value) => {
                values.insert("until".into(), value.format("%Y%m%dT%H%M%SZ").to_string());
            }
        }
        values
    }
}

fn number(fields: &BTreeMap<String, String>, key: &str, default: u16) -> Result<u16> {
    fields.get(key).map_or(Ok(default), |value| value.parse().map_err(|_| invalid()))
}

fn small(fields: &BTreeMap<String, String>, key: &str) -> Result<u8> {
    fields.get(key).ok_or_else(invalid)?.parse().map_err(|_| invalid())
}

fn invalid() -> EasError {
    EasError::InvalidConfiguration("Calendar recurrence cannot be preserved safely".into())
}
