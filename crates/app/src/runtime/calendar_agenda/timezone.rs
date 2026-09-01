use base64::Engine as _;
use chrono::{DateTime, Datelike as _, Duration, NaiveDate, NaiveDateTime, Utc};
use chrono_tz::Tz;

use crate::{AppError, ErrorCode, Result};

const TIME_ZONE_BYTES: usize = 172;
const MAX_OFFSET_MINUTES: i32 = 24 * 60;

pub(in crate::runtime) enum EventTimeZone {
    Eas(EasTimeZone),
    Iana(Tz),
}

impl EventTimeZone {
    pub(in crate::runtime) fn parse(encoded: Option<&str>, fallback: Tz) -> Result<Self> {
        let Some(encoded) = encoded.filter(|value| !value.is_empty()) else {
            return Ok(Self::Iana(fallback));
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| protocol("Calendar timezone is not valid base64"))?;
        Ok(Self::Eas(EasTimeZone::parse(&bytes)?))
    }

    pub(in crate::runtime) fn icalendar(
        &self,
        id: &str,
        year: i32,
    ) -> Result<icalendar::CalendarComponent> {
        use icalendar::{Property, parser::Component};
        let Self::Eas(zone) = self else {
            return Err(protocol("Calendar notification requires an EAS timezone"));
        };
        let mut output =
            Component { name: "VTIMEZONE".into(), properties: Vec::new(), components: Vec::new() };
        push_property(&mut output, Property::new("TZID", id));
        for (name, rule, from, to) in [
            ("STANDARD", zone.standard_transition, zone.daylight_offset, zone.standard_offset),
            ("DAYLIGHT", zone.daylight_transition, zone.standard_offset, zone.daylight_offset),
        ] {
            if name == "DAYLIGHT" && rule.is_none() {
                continue;
            }
            let mut child =
                Component { name: name.into(), properties: Vec::new(), components: Vec::new() };
            let start = if let Some(rule) = rule {
                rule.in_year(year)?
            } else {
                NaiveDate::from_ymd_opt(year, 1, 1)
                    .and_then(|date| date.and_hms_opt(0, 0, 0))
                    .ok_or_else(|| protocol("Calendar timezone year is invalid"))?
            };
            push_property(
                &mut child,
                Property::new("DTSTART", start.format("%Y%m%dT%H%M%S").to_string()),
            );
            push_property(
                &mut child,
                Property::new("TZOFFSETFROM", ical_offset(if rule.is_none() { to } else { from })),
            );
            push_property(&mut child, Property::new("TZOFFSETTO", ical_offset(to)));
            if let Some(rule) = rule {
                let ordinal = if rule.week == 5 { -1 } else { i64::from(rule.week) };
                let weekday = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"]
                    .get(
                        usize::try_from(rule.weekday_from_sunday)
                            .map_err(|_| protocol("invalid weekday"))?,
                    )
                    .ok_or_else(|| protocol("invalid weekday"))?;
                push_property(
                    &mut child,
                    Property::new(
                        "RRULE",
                        format!("FREQ=YEARLY;BYMONTH={};BYDAY={ordinal}{weekday}", rule.month),
                    ),
                );
            }
            output.components.push(child);
        }
        Ok(output.into())
    }

    pub(in crate::runtime) fn to_local(&self, value: DateTime<Utc>) -> Result<NaiveDateTime> {
        match self {
            Self::Eas(zone) => zone.to_local(value),
            Self::Iana(zone) => Ok(value.with_timezone(zone).naive_local()),
        }
    }

    pub(in crate::runtime) fn to_utc(&self, value: NaiveDateTime) -> Result<DateTime<Utc>> {
        match self {
            Self::Eas(zone) => zone.to_utc(value),
            Self::Iana(zone) => super::local_to_utc(*zone, value),
        }
    }
}

pub(in crate::runtime) struct EasTimeZone {
    standard_offset: i32,
    daylight_offset: i32,
    standard_transition: Option<TransitionRule>,
    daylight_transition: Option<TransitionRule>,
}

impl EasTimeZone {
    fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != TIME_ZONE_BYTES {
            return Err(protocol("Calendar timezone must contain 172 bytes"));
        }
        let bias = read_i32(bytes, 0)?;
        let standard_bias = read_i32(bytes, 84)?;
        let daylight_bias = read_i32(bytes, 168)?;
        let standard_offset = offset(bias, standard_bias)?;
        let daylight_offset = offset(bias, daylight_bias)?;
        let standard_transition = TransitionRule::parse(bytes, 68)?;
        let daylight_transition = TransitionRule::parse(bytes, 152)?;
        if standard_transition.is_some() != daylight_transition.is_some() {
            return Err(protocol("Calendar timezone has an incomplete DST rule"));
        }
        Ok(Self { standard_offset, daylight_offset, standard_transition, daylight_transition })
    }

    fn to_local(&self, value: DateTime<Utc>) -> Result<NaiveDateTime> {
        let offset =
            if self.is_daylight_utc(value)? { self.daylight_offset } else { self.standard_offset };
        value
            .naive_utc()
            .checked_add_signed(Duration::seconds(i64::from(offset)))
            .ok_or_else(|| protocol("Calendar timezone conversion overflowed"))
    }

    fn to_utc(&self, value: NaiveDateTime) -> Result<DateTime<Utc>> {
        let mut candidates = Vec::new();
        for offset in [self.standard_offset, self.daylight_offset] {
            let naive = value
                .checked_sub_signed(Duration::seconds(i64::from(offset)))
                .ok_or_else(|| protocol("Calendar timezone conversion overflowed"))?;
            let instant = DateTime::from_naive_utc_and_offset(naive, Utc);
            if self.to_local(instant)? == value && !candidates.contains(&instant) {
                candidates.push(instant);
            }
        }
        match candidates.as_slice() {
            [instant] => Ok(*instant),
            [] => Err(protocol("Calendar local time does not exist")),
            _ => Err(protocol("Calendar local time is ambiguous")),
        }
    }

    fn is_daylight_utc(&self, value: DateTime<Utc>) -> Result<bool> {
        let Some(daylight) = self.daylight_transition else {
            return Ok(false);
        };
        let standard = self
            .standard_transition
            .ok_or_else(|| protocol("Calendar timezone has no standard transition"))?;
        let year = value.year();
        let daylight = daylight
            .in_year(year)?
            .checked_sub_signed(Duration::seconds(i64::from(self.standard_offset)))
            .ok_or_else(|| protocol("Calendar DST transition overflowed"))?;
        let standard = standard
            .in_year(year)?
            .checked_sub_signed(Duration::seconds(i64::from(self.daylight_offset)))
            .ok_or_else(|| protocol("Calendar DST transition overflowed"))?;
        let value = value.naive_utc();
        Ok(if daylight < standard {
            value >= daylight && value < standard
        } else {
            value >= daylight || value < standard
        })
    }
}

#[derive(Clone, Copy)]
struct TransitionRule {
    month: u32,
    weekday_from_sunday: u32,
    week: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millisecond: u32,
}

impl TransitionRule {
    fn parse(bytes: &[u8], offset: usize) -> Result<Option<Self>> {
        let month = u32::from(read_u16(bytes, offset + 2)?);
        if month == 0 {
            return Ok(None);
        }
        let rule = Self {
            month,
            weekday_from_sunday: u32::from(read_u16(bytes, offset + 4)?),
            week: u32::from(read_u16(bytes, offset + 6)?),
            hour: u32::from(read_u16(bytes, offset + 8)?),
            minute: u32::from(read_u16(bytes, offset + 10)?),
            second: u32::from(read_u16(bytes, offset + 12)?),
            millisecond: u32::from(read_u16(bytes, offset + 14)?),
        };
        if !(1..=12).contains(&rule.month)
            || rule.weekday_from_sunday > 6
            || !(1..=5).contains(&rule.week)
        {
            return Err(protocol("Calendar timezone contains an invalid DST rule"));
        }
        Ok(Some(rule))
    }

    fn in_year(self, year: i32) -> Result<NaiveDateTime> {
        let first = NaiveDate::from_ymd_opt(year, self.month, 1)
            .ok_or_else(|| protocol("Calendar DST transition date is invalid"))?;
        let first_weekday = first.weekday().num_days_from_sunday();
        let delta = (7 + self.weekday_from_sunday - first_weekday) % 7;
        let date = if self.week < 5 {
            first.checked_add_signed(Duration::days(i64::from(delta + 7 * (self.week - 1))))
        } else {
            let next_month = if self.month == 12 {
                NaiveDate::from_ymd_opt(year + 1, 1, 1)
            } else {
                NaiveDate::from_ymd_opt(year, self.month + 1, 1)
            }
            .ok_or_else(|| protocol("Calendar DST transition date is invalid"))?;
            let last = next_month
                .pred_opt()
                .ok_or_else(|| protocol("Calendar DST transition date is invalid"))?;
            let back = (7 + last.weekday().num_days_from_sunday() - self.weekday_from_sunday) % 7;
            last.checked_sub_signed(Duration::days(i64::from(back)))
        }
        .ok_or_else(|| protocol("Calendar DST transition date overflowed"))?;
        date.and_hms_milli_opt(self.hour, self.minute, self.second, self.millisecond)
            .ok_or_else(|| protocol("Calendar DST transition time is invalid"))
    }
}

fn push_property(
    component: &mut icalendar::parser::Component<'static>,
    property: icalendar::Property,
) {
    component.properties.push(property.into());
}

fn ical_offset(seconds: i32) -> String {
    let sign = if seconds < 0 { "-" } else { "+" };
    let seconds = seconds.unsigned_abs();
    format!("{sign}{:02}{:02}", seconds / 3600, (seconds % 3600) / 60)
}

fn offset(bias: i32, seasonal_bias: i32) -> Result<i32> {
    let minutes = bias
        .checked_add(seasonal_bias)
        .and_then(i32::checked_neg)
        .ok_or_else(|| protocol("Calendar timezone offset overflowed"))?;
    if minutes.abs() > MAX_OFFSET_MINUTES {
        return Err(protocol("Calendar timezone offset is outside supported bounds"));
    }
    minutes.checked_mul(60).ok_or_else(|| protocol("Calendar timezone offset overflowed"))
}

fn read_i32(bytes: &[u8], offset: usize) -> Result<i32> {
    let value = bytes
        .get(offset..offset + 4)
        .and_then(|value| value.try_into().ok())
        .ok_or_else(|| protocol("Calendar timezone is truncated"))?;
    Ok(i32::from_le_bytes(value))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let value = bytes
        .get(offset..offset + 2)
        .and_then(|value| value.try_into().ok())
        .ok_or_else(|| protocol("Calendar timezone is truncated"))?;
    Ok(u16::from_le_bytes(value))
}

fn protocol(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ProtocolError, message)
}
