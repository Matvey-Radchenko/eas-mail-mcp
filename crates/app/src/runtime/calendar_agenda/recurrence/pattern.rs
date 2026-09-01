use std::collections::BTreeMap;

use chrono::{DateTime, Datelike as _, Duration, NaiveDate, NaiveDateTime, Utc};

use super::super::protocol;
use crate::{AppError, ErrorCode, Result};

pub(in crate::runtime) struct Pattern {
    kind: u8,
    interval: u32,
    day_mask: Option<u32>,
    day_of_month: Option<u32>,
    week_of_month: Option<u32>,
    month_of_year: Option<u32>,
    first_weekday: u32,
    occurrences: Option<u32>,
    pub(in crate::runtime) until: Option<DateTime<Utc>>,
}

impl Pattern {
    pub(in crate::runtime) fn parse(values: &BTreeMap<String, String>) -> Result<Self> {
        let calendar_type = number(values, "calendartype")?.unwrap_or(1);
        if !matches!(calendar_type, 0 | 1) {
            return Err(AppError::new(
                ErrorCode::FeatureUnavailable,
                "Non-Gregorian Calendar recurrence is not supported",
            ));
        }
        let kind = u8::try_from(required_number(values, "type")?)
            .map_err(|_| protocol("Calendar recurrence type is invalid"))?;
        if !matches!(kind, 0 | 1 | 2 | 3 | 5 | 6) {
            return Err(protocol("Calendar recurrence type is unsupported"));
        }
        let interval = number(values, "interval")?.unwrap_or(1).max(1);
        if interval > 999 {
            return Err(protocol("Calendar recurrence interval is invalid"));
        }
        let first_weekday = number(values, "firstdayofweek")?.unwrap_or(0);
        if first_weekday > 6 {
            return Err(protocol("Calendar recurrence first weekday is invalid"));
        }
        let day_mask = number(values, "dayofweek")?;
        if day_mask.is_some_and(|value| value == 0 || value > 127) {
            return Err(protocol("Calendar recurrence weekday mask is invalid"));
        }
        Ok(Self {
            kind,
            interval,
            day_mask,
            day_of_month: number(values, "dayofmonth")?,
            week_of_month: number(values, "weekofmonth")?,
            month_of_year: number(values, "monthofyear")?,
            first_weekday,
            occurrences: number(values, "occurrences")?,
            until: values.get("until").map(|value| parse_time(value)).transpose()?,
        })
    }

    pub(in crate::runtime) fn ordinal(
        &self,
        date: NaiveDate,
        master: NaiveDate,
    ) -> Result<Option<u32>> {
        if date < master {
            return Ok(None);
        }
        match self.kind {
            0 if self.day_mask.is_none() => self.daily(date, master),
            0 | 1 => self.weekly(date, master),
            2 => self.monthly(date, master, false),
            3 => self.monthly(date, master, true),
            5 => self.yearly(date, master, false),
            6 => self.yearly(date, master, true),
            _ => Err(protocol("Calendar recurrence type is unsupported")),
        }
    }

    pub(in crate::runtime) fn allows(&self, ordinal: u32) -> bool {
        self.occurrences.is_none_or(|maximum| ordinal <= maximum)
    }

    fn daily(&self, date: NaiveDate, master: NaiveDate) -> Result<Option<u32>> {
        let days = date.signed_duration_since(master).num_days();
        let interval = i64::from(self.interval);
        if days % interval != 0 {
            return Ok(None);
        }
        u32::try_from(days / interval + 1)
            .map(Some)
            .map_err(|_| protocol("Calendar recurrence ordinal overflowed"))
    }

    fn weekly(&self, date: NaiveDate, master: NaiveDate) -> Result<Option<u32>> {
        let mask = self.day_mask.ok_or_else(|| protocol("Weekly recurrence has no weekdays"))?;
        if mask & weekday_bit(date) == 0 {
            return Ok(None);
        }
        let anchor = week_start(master, self.first_weekday)?;
        let candidate = week_start(date, self.first_weekday)?;
        let weeks = candidate.signed_duration_since(anchor).num_days() / 7;
        let interval = i64::from(self.interval);
        if weeks < 0 || weeks % interval != 0 {
            return Ok(None);
        }
        let cycle = u32::try_from(weeks / interval)
            .map_err(|_| protocol("Calendar recurrence ordinal overflowed"))?;
        let first = selected_weekdays(anchor, mask)
            .into_iter()
            .filter(|value| *value >= master)
            .collect::<Vec<_>>();
        if cycle == 0 {
            return Ok(position(&first, date));
        }
        let current = selected_weekdays(candidate, mask);
        let position =
            position(&current, date).ok_or_else(|| protocol("Calendar weekday is missing"))?;
        let per_cycle = mask.count_ones();
        let prior = u32::try_from(first.len())
            .ok()
            .and_then(|first| cycle.checked_sub(1)?.checked_mul(per_cycle)?.checked_add(first))
            .and_then(|value| value.checked_add(position))
            .ok_or_else(|| protocol("Calendar recurrence ordinal overflowed"))?;
        Ok(Some(prior))
    }

    fn monthly(&self, date: NaiveDate, master: NaiveDate, relative: bool) -> Result<Option<u32>> {
        let months = month_delta(master, date);
        if months < 0 || months % i64::from(self.interval) != 0 {
            return Ok(None);
        }
        let expected = if relative {
            nth_date(
                date.year(),
                date.month(),
                self.day_mask.ok_or_else(|| protocol("Monthly recurrence has no weekdays"))?,
                self.week_of_month
                    .ok_or_else(|| protocol("Monthly recurrence has no week index"))?,
            )?
        } else {
            calendar_day(
                date.year(),
                date.month(),
                self.day_of_month
                    .ok_or_else(|| protocol("Monthly recurrence has no day of month"))?,
            )
        };
        self.calendar_ordinal(expected, date, master, relative, false)
    }

    fn yearly(&self, date: NaiveDate, master: NaiveDate, relative: bool) -> Result<Option<u32>> {
        let years = i64::from(date.year()) - i64::from(master.year());
        if years < 0 || years % i64::from(self.interval) != 0 {
            return Ok(None);
        }
        let month = self.month_of_year.ok_or_else(|| protocol("Yearly recurrence has no month"))?;
        let expected = if relative {
            nth_date(
                date.year(),
                month,
                self.day_mask.unwrap_or_else(|| weekday_bit(master)),
                self.week_of_month
                    .ok_or_else(|| protocol("Yearly recurrence has no week index"))?,
            )?
        } else {
            calendar_day(
                date.year(),
                month,
                self.day_of_month
                    .ok_or_else(|| protocol("Yearly recurrence has no day of month"))?,
            )
        };
        self.calendar_ordinal(expected, date, master, relative, true)
    }
    fn calendar_ordinal(
        &self,
        expected: Option<NaiveDate>,
        date: NaiveDate,
        master: NaiveDate,
        relative: bool,
        yearly: bool,
    ) -> Result<Option<u32>> {
        if expected != Some(date) {
            return Ok(None);
        }
        let stride = i64::from(self.interval) * if yearly { 12 } else { 1 };
        let cycles = month_delta(master, date) / stride;
        if cycles > 120_000 {
            return Err(protocol("Calendar recurrence range is too large"));
        }
        let anchor = i64::from(master.year()) * 12 + i64::from(master.month0());
        let mut count = 0;
        for index in 0..=cycles {
            let absolute = anchor + index * stride;
            let year = i32::try_from(absolute.div_euclid(12))
                .map_err(|_| protocol("Calendar year overflowed"))?;
            let month = if yearly {
                self.month_of_year.unwrap_or(master.month())
            } else {
                u32::try_from(absolute.rem_euclid(12) + 1)
                    .map_err(|_| protocol("Calendar month overflowed"))?
            };
            let candidate = if relative {
                nth_date(
                    year,
                    month,
                    self.day_mask.unwrap_or_else(|| weekday_bit(master)),
                    self.week_of_month.ok_or_else(|| protocol("Calendar ordinal is missing"))?,
                )?
            } else {
                calendar_day(
                    year,
                    month,
                    self.day_of_month.ok_or_else(|| protocol("Calendar day is missing"))?,
                )
            };
            if candidate.is_some_and(|candidate| candidate >= master && candidate <= date) {
                count += 1;
            }
        }
        Ok(Some(count))
    }
}

fn exact_date(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(year, month, day)
}

fn calendar_day(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
    if !(1..=31).contains(&day) {
        return None;
    }
    // Exchange clamps a numbered date to month-end; RFC BYMONTHDAY alone would skip it.
    let next =
        if month == 12 { exact_date(year + 1, 1, 1) } else { exact_date(year, month + 1, 1) }?;
    let last = next.pred_opt()?.day();
    exact_date(year, month, day.min(last))
}

fn nth_date(year: i32, month: u32, mask: u32, nth: u32) -> Result<Option<NaiveDate>> {
    if mask == 127 {
        return Ok(if nth == 5 {
            calendar_day(year, month, 31)
        } else {
            exact_date(year, month, nth)
        });
    }
    if !(1..=5).contains(&nth) || mask == 0 || mask > 127 {
        return Err(protocol("Calendar recurrence monthly selector is invalid"));
    }
    let first = exact_date(year, month, 1)
        .ok_or_else(|| protocol("Calendar recurrence month is invalid"))?;
    let next =
        if month == 12 { exact_date(year + 1, 1, 1) } else { exact_date(year, month + 1, 1) }
            .ok_or_else(|| protocol("Calendar recurrence month is invalid"))?;
    let mut matches = Vec::new();
    let mut date = first;
    while date < next {
        if mask & weekday_bit(date) != 0 {
            matches.push(date);
        }
        date = date.succ_opt().ok_or_else(|| protocol("Calendar recurrence date overflowed"))?;
    }
    let index = if nth == 5 { matches.len().checked_sub(1) } else { usize::try_from(nth - 1).ok() };
    Ok(index.and_then(|index| matches.get(index).copied()))
}

fn selected_weekdays(start: NaiveDate, mask: u32) -> Vec<NaiveDate> {
    (0..7)
        .filter_map(|days| start.checked_add_signed(Duration::days(days)))
        .filter(|date| mask & weekday_bit(*date) != 0)
        .collect()
}

fn position(values: &[NaiveDate], date: NaiveDate) -> Option<u32> {
    values.iter().position(|value| *value == date).and_then(|value| u32::try_from(value + 1).ok())
}

fn week_start(date: NaiveDate, first_weekday: u32) -> Result<NaiveDate> {
    let current = date.weekday().num_days_from_sunday();
    let days = (7 + current - first_weekday) % 7;
    date.checked_sub_signed(Duration::days(i64::from(days)))
        .ok_or_else(|| protocol("Calendar recurrence week underflowed"))
}

fn weekday_bit(date: NaiveDate) -> u32 {
    1_u32 << date.weekday().num_days_from_sunday()
}

fn month_delta(start: NaiveDate, end: NaiveDate) -> i64 {
    (i64::from(end.year()) * 12 + i64::from(end.month()))
        - (i64::from(start.year()) * 12 + i64::from(start.month()))
}

fn required_number(values: &BTreeMap<String, String>, key: &str) -> Result<u32> {
    number(values, key)?.ok_or_else(|| protocol("Calendar recurrence is missing a required field"))
}

fn number(values: &BTreeMap<String, String>, key: &str) -> Result<Option<u32>> {
    values
        .get(key)
        .map(|value| value.parse().map_err(|_| protocol("Calendar recurrence number is invalid")))
        .transpose()
}

fn parse_time(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .or_else(|_| {
            NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%SZ")
                .map(|value| DateTime::from_naive_utc_and_offset(value, Utc))
        })
        .map_err(|_| protocol("Calendar recurrence timestamp is invalid"))
}
