use eas_mail_protocol::{CalendarRecurrence, RecurrenceEnd, RecurrencePattern};

pub(in crate::runtime) fn recurrence(value: Option<&CalendarRecurrence>) -> String {
    let Some(rule) = value else {
        return "none".into();
    };
    let interval = rule.interval;
    let pattern = match rule.pattern {
        RecurrencePattern::Daily => format!("every {interval} day(s)"),
        RecurrencePattern::Weekly { days } => {
            format!("every {interval} week(s) on {}", weekdays(days))
        }
        RecurrencePattern::Monthly { day } => {
            format!("every {interval} month(s), day {day} (month-end if shorter)")
        }
        RecurrencePattern::MonthlyRelative { days, week } => {
            format!("every {interval} month(s), {}", relative(days, week))
        }
        RecurrencePattern::Yearly { month, day } => {
            format!("every {interval} year(s), month {month}, day {day} (month-end if shorter)")
        }
        RecurrencePattern::YearlyRelative { month, days, week } => {
            format!("every {interval} year(s), month {month}, {}", relative(days, week))
        }
    };
    let ending = match rule.end {
        RecurrenceEnd::Never => "no end date".into(),
        RecurrenceEnd::Count(count) => format!("{count} occurrences in total"),
        RecurrenceEnd::Until(until) => format!("through {} inclusive", until.to_rfc3339()),
    };
    format!("{pattern}; {ending}; dates follow the event's local timezone")
}

fn relative(days: u8, week: u8) -> String {
    let ordinal = match week {
        1 => "first",
        2 => "second",
        3 => "third",
        4 => "fourth",
        _ => "last",
    };
    if days == 127 {
        format!("{ordinal} day")
    } else {
        format!("{ordinal} matching day of {}", weekdays(days))
    }
}

fn weekdays(mask: u8) -> String {
    ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
        .iter()
        .enumerate()
        .filter(|(index, _)| mask & (1 << index) != 0)
        .map(|(_, day)| *day)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_and_endings_are_readable_without_protocol_numbers() {
        for pattern in [
            RecurrencePattern::Daily,
            RecurrencePattern::Weekly { days: 10 },
            RecurrencePattern::Monthly { day: 31 },
            RecurrencePattern::MonthlyRelative { days: 127, week: 5 },
            RecurrencePattern::Yearly { month: 2, day: 29 },
            RecurrencePattern::YearlyRelative { month: 8, days: 62, week: 2 },
        ] {
            for end in [
                RecurrenceEnd::Never,
                RecurrenceEnd::Count(5),
                RecurrenceEnd::Until(chrono::DateTime::UNIX_EPOCH),
            ] {
                let rule = CalendarRecurrence {
                    pattern: pattern.clone(),
                    interval: 2,
                    first_day_of_week: 1,
                    end,
                };
                let text = recurrence(Some(&rule));
                assert!(text.starts_with("every 2"));
                assert!(text.contains("local timezone"));
                assert!(!text.contains("{\"type\""));
            }
        }
        assert_eq!(recurrence(None), "none");
    }
}
