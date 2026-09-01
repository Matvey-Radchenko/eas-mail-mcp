use crate::wbxml::Element;
use crate::{CalendarException, CalendarProperties, EasError, Patch, Result};

use super::tree::{element, push_text};

pub(super) fn append(parent: &mut Element, properties: &CalendarProperties) -> Result<()> {
    if !properties.can_write() {
        return Err(EasError::InvalidConfiguration(
            "Calendar data cannot be preserved safely".into(),
        ));
    }
    if let Some(sensitivity) = properties.sensitivity {
        push_text(parent, "Calendar", "Sensitivity", sensitivity.to_string());
    }
    if let Some(values) = &properties.categories {
        let mut categories = element("Calendar", "Categories");
        for category in values {
            push_text(&mut categories, "Calendar", "Category", category);
        }
        parent.push(categories);
    }
    if let Some(rule) = &properties.recurrence {
        rule.validate()?;
        let mut recurrence = element("Calendar", "Recurrence");
        let fields = rule.to_fields();
        for (key, tag) in [
            ("type", "Type"),
            ("until", "Until"),
            ("occurrences", "Occurrences"),
            ("interval", "Interval"),
            ("dayofweek", "DayOfWeek"),
            ("dayofmonth", "DayOfMonth"),
            ("weekofmonth", "WeekOfMonth"),
            ("monthofyear", "MonthOfYear"),
            ("firstdayofweek", "FirstDayOfWeek"),
        ] {
            if let Some(value) = fields.get(key) {
                push_text(&mut recurrence, "Calendar", tag, value);
            }
        }
        parent.push(recurrence);
        let mut exceptions = element("Calendar", "Exceptions");
        for exception in &properties.exceptions {
            exceptions.push(render_exception(exception)?);
        }
        parent.push(exceptions);
    }
    Ok(())
}

fn render_exception(value: &CalendarException) -> Result<Element> {
    let mut output = element("Calendar", "Exception");
    push_text(&mut output, "Calendar", "Deleted", u8::from(value.deleted).to_string());
    push_text(
        &mut output,
        "Calendar",
        "ExceptionStartTime",
        value.original_start.format("%Y%m%dT%H%M%SZ").to_string(),
    );
    if value.deleted {
        return Ok(output);
    }
    let fields = &value.fields;
    for (tag, field) in [("Subject", &fields.subject), ("Location", &fields.location)] {
        if let Patch::Value(value) = field {
            push_text(&mut output, "Calendar", tag, value);
        }
    }
    for (tag, field) in [
        ("StartTime", &fields.starts_at),
        ("EndTime", &fields.ends_at),
        ("DtStamp", &fields.dt_stamp),
    ] {
        if let Patch::Value(Some(value)) = field {
            push_text(&mut output, "Calendar", tag, value.format("%Y%m%dT%H%M%SZ").to_string());
        }
    }
    bool_field(&mut output, "AllDayEvent", &fields.all_day);
    number_field(&mut output, "BusyStatus", &fields.busy_status);
    number_field(&mut output, "MeetingStatus", &fields.meeting_status);
    number_field(&mut output, "Reminder", &fields.reminder_minutes);
    if let Patch::Value(body) = &fields.body {
        let mut container = element("AirSyncBase", "Body");
        push_text(&mut container, "AirSyncBase", "Type", "1");
        push_text(&mut container, "AirSyncBase", "Data", body);
        output.push(container);
    }
    if let Patch::Value(attendees) = &fields.attendees {
        output.push(super::calendar_mutation::attendees(attendees));
    }
    if let Some(properties) = &fields.properties {
        append(&mut output, properties)?;
    }
    Ok(output)
}

fn bool_field(parent: &mut Element, tag: &str, value: &Patch<bool>) {
    if let Patch::Value(value) = value {
        push_text(parent, "Calendar", tag, u8::from(*value).to_string());
    }
}

fn number_field<T: ToString>(parent: &mut Element, tag: &str, value: &Patch<T>) {
    if let Patch::Value(value) = value {
        push_text(parent, "Calendar", tag, value.to_string());
    }
}
