use chrono::{Datelike as _, NaiveDate};
use eas_mail_protocol::{CalendarApplication, RecurrenceEnd, RecurrencePattern};
use icalendar::{Calendar, Component as _, Event, EventLike as _, Property};

use super::calendar_series;
use crate::Result;

pub(super) fn schedule(
    event: &mut Event,
    item: &CalendarApplication,
    dates: Option<(NaiveDate, NaiveDate)>,
) -> Result<()> {
    let recurring =
        item.properties.recurrence.is_some() || item.properties.instance_start.is_some();
    if let Some((start, end)) = dates {
        event.starts(start).ends(end);
    } else if recurring {
        event.append_property(local_property("DTSTART", item.starts_at, item)?);
        event.append_property(local_property("DTEND", item.ends_at, item)?);
    } else {
        event.starts(item.starts_at).ends(item.ends_at);
    }
    if let Some(original) = item.properties.instance_start {
        let property = if item.properties.instance_all_day.unwrap_or(item.all_day) {
            Property::new(
                "RECURRENCE-ID",
                calendar_series::zone(item)?.to_local(original)?.format("%Y%m%d").to_string(),
            )
            .add_parameter("VALUE", "DATE")
            .done()
        } else {
            local_property("RECURRENCE-ID", original, item)?
        };
        event.append_property(property);
    } else if let Some(rule) = &item.properties.recurrence {
        rule.validate()?;
        let mut fields = recurrence(rule);
        match rule.end {
            RecurrenceEnd::Never => {}
            RecurrenceEnd::Count(count) => fields.push(format!("COUNT={count}")),
            RecurrenceEnd::Until(until) => fields.push(format!(
                "UNTIL={}",
                if item.all_day {
                    calendar_series::zone(item)?.to_local(until)?.format("%Y%m%d").to_string()
                } else {
                    until.format("%Y%m%dT%H%M%SZ").to_string()
                }
            )),
        }
        event.append_property(Property::new("RRULE", fields.join(";")));
        for exception in item.properties.exceptions.iter().filter(|value| value.deleted) {
            let property = if item.all_day {
                Property::new(
                    "EXDATE",
                    calendar_series::zone(item)?
                        .to_local(exception.original_start)?
                        .format("%Y%m%d")
                        .to_string(),
                )
                .add_parameter("VALUE", "DATE")
                .done()
            } else {
                local_property("EXDATE", exception.original_start, item)?
            };
            event.append_multi_property(property);
        }
    }
    Ok(())
}

pub(super) fn timezone(calendar: &mut Calendar, item: &CalendarApplication) -> Result<()> {
    let timed_exception = item.properties.exceptions.iter().any(|exception| {
        !exception.deleted
            && matches!(exception.fields.all_day, eas_mail_protocol::Patch::Value(false))
    });
    if (!item.all_day || item.properties.instance_all_day == Some(false) || timed_exception)
        && (item.properties.recurrence.is_some() || item.properties.instance_start.is_some())
    {
        let zone = calendar_series::zone(item)?;
        calendar.push(zone.icalendar(&zone_id(item), item.starts_at.year())?);
    }
    Ok(())
}

fn local_property(
    name: &str,
    instant: chrono::DateTime<chrono::Utc>,
    item: &CalendarApplication,
) -> Result<Property> {
    let value = calendar_series::zone(item)?.to_local(instant)?.format("%Y%m%dT%H%M%S").to_string();
    Ok(Property::new(name, value).add_parameter("TZID", &zone_id(item)).done())
}

fn zone_id(item: &CalendarApplication) -> String {
    use sha2::{Digest as _, Sha256};
    let digest = Sha256::digest(item.time_zone.as_bytes());
    format!(
        "EAS-{}",
        digest.iter().take(12).map(|value| format!("{value:02x}")).collect::<String>()
    )
}

fn recurrence(rule: &eas_mail_protocol::CalendarRecurrence) -> Vec<String> {
    let mut fields = match rule.pattern {
        RecurrencePattern::Daily => vec!["FREQ=DAILY".into()],
        RecurrencePattern::Weekly { days } => {
            vec!["FREQ=WEEKLY".into(), format!("BYDAY={}", weekdays(days))]
        }
        RecurrencePattern::Monthly { day } => {
            let mut values = vec!["FREQ=MONTHLY".into()];
            values.extend(month_day(day));
            values
        }
        RecurrencePattern::MonthlyRelative { days, week } => relative("MONTHLY", days, week),
        RecurrencePattern::Yearly { month, day } => {
            let mut values = vec!["FREQ=YEARLY".into(), format!("BYMONTH={month}")];
            values.extend(month_day(day));
            values
        }
        RecurrencePattern::YearlyRelative { month, days, week } => {
            let mut fields = relative("YEARLY", days, week);
            fields.push(format!("BYMONTH={month}"));
            fields
        }
    };
    fields.push(format!("INTERVAL={}", rule.interval));
    let weekday = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"]
        .get(usize::from(rule.first_day_of_week))
        .copied()
        .unwrap_or("SU");
    fields.push(format!("WKST={weekday}"));
    fields
}

fn relative(frequency: &str, days: u8, week: u8) -> Vec<String> {
    if days == 127 {
        return vec![
            format!("FREQ={frequency}"),
            format!("BYMONTHDAY={}", if week == 5 { -1 } else { i16::from(week) }),
        ];
    }
    vec![
        format!("FREQ={frequency}"),
        format!("BYDAY={}", weekdays(days)),
        format!("BYSETPOS={}", if week == 5 { -1 } else { i16::from(week) }),
    ]
}

fn month_day(day: u8) -> Vec<String> {
    match day {
        29 | 30 => vec![
            format!(
                "BYMONTHDAY={}",
                (28..=day).map(|value| value.to_string()).collect::<Vec<_>>().join(",")
            ),
            "BYSETPOS=-1".into(),
        ],
        31 => vec!["BYMONTHDAY=-1".into()],
        _ => vec![format!("BYMONTHDAY={day}")],
    }
}

fn weekdays(mask: u8) -> String {
    ["SU", "MO", "TU", "WE", "TH", "FR", "SA"]
        .iter()
        .enumerate()
        .filter(|(index, _)| mask & (1 << index) != 0)
        .map(|(_, value)| *value)
        .collect::<Vec<_>>()
        .join(",")
}
