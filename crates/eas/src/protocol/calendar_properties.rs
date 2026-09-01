use std::collections::{BTreeMap, BTreeSet};

use crate::wbxml::Element;
use crate::{CalendarException, CalendarProperties, CalendarRecurrence, Patch};

use super::sync::parse_calendar_fields_base;
use super::tree::{direct_text, parse_datetime};

pub(super) fn parse(parent: &Element) -> CalendarProperties {
    let mut output = common(parent);
    if let Some(rule) = parent.child("Calendar", "Recurrence") {
        let fields: BTreeMap<_, _> = rule
            .children()
            .map(|value| (value.name.to_ascii_lowercase(), value.text_content()))
            .collect();
        match CalendarRecurrence::from_fields(&fields) {
            Ok(value)
                if fields.len() == rule.children().count()
                    && rule.children().all(|child| {
                        child.namespace == "Calendar" && child.children().next().is_none()
                    }) =>
            {
                output.recurrence = Some(value)
            }
            _ => output.unsupported = true,
        }
    }
    if let Some(exceptions) = parent.child("Calendar", "Exceptions") {
        let mut seen = BTreeSet::new();
        for exception in exceptions.children() {
            if output.exceptions.len() >= 256
                || exception.name != "Exception"
                || exception.namespace != "Calendar"
            {
                output.unsupported = true;
                continue;
            }
            let Some(start) =
                parse_datetime(direct_text(exception, "Calendar", "ExceptionStartTime"))
            else {
                output.unsupported = true;
                continue;
            };
            if !seen.insert(start) {
                output.unsupported = true;
            }
            let mut fields = parse_calendar_fields_base(exception);
            let mut properties = common(exception);
            properties.unsupported |= exception.children().any(|child| {
                child.namespace == "Calendar"
                    && matches!(
                        child.name.as_str(),
                        "Recurrence" | "Exceptions" | "TimeZone" | "UID"
                    )
            });
            fields.properties = Some(properties);
            output.exceptions.push(CalendarException {
                original_start: start,
                deleted: direct_text(exception, "Calendar", "Deleted").as_deref() == Some("1"),
                fields,
            });
        }
    }
    output
}

fn common(parent: &Element) -> CalendarProperties {
    let mut output = CalendarProperties {
        unsupported: !super::calendar_validation::fields_supported(parent),
        ..Default::default()
    };
    const CALENDAR: &[&str] = &[
        "TimeZone",
        "AllDayEvent",
        "Attendees",
        "BusyStatus",
        "DtStamp",
        "EndTime",
        "Location",
        "MeetingStatus",
        "OrganizerEmail",
        "OrganizerName",
        "Recurrence",
        "Reminder",
        "Sensitivity",
        "Subject",
        "StartTime",
        "UID",
        "Exceptions",
        "ResponseRequested",
        "ResponseType",
        "Categories",
        "ExceptionStartTime",
        "Deleted",
        "AppointmentReplyTime",
        "NativeBodyType",
    ];
    let mut seen = BTreeSet::new();
    for child in parent.children() {
        let known = match child.namespace.as_str() {
            "Calendar" => CALENDAR.contains(&child.name.as_str()),
            "AirSyncBase" => matches!(child.name.as_str(), "Body" | "NativeBodyType"),
            "AirSync" | "Search" => {
                matches!(child.name.as_str(), "Class" | "CollectionId" | "ServerId" | "LongId")
            }
            _ => false,
        };
        output.unsupported |= !known || !seen.insert((&child.namespace, &child.name));
    }
    if let Some(value) = direct_text(parent, "Calendar", "Sensitivity") {
        match value.parse::<u8>() {
            Ok(value @ 0..=3) => output.sensitivity = Some(value),
            _ => output.unsupported = true,
        }
    }
    if let Some(categories) = parent.child("Calendar", "Categories") {
        let mut values = Vec::new();
        for child in categories.children() {
            if child.namespace != "Calendar" || child.name != "Category" {
                output.unsupported = true;
            }
            values.push(child.text_content());
        }
        output.categories = Some(values);
    }
    if let Some(body) = parent.child("AirSyncBase", "Body") {
        output.unsupported |= direct_text(body, "AirSyncBase", "Truncated").as_deref() == Some("1");
    }
    output
}

/// Creates the legacy exception projection without discarding the internal typed fields.
pub fn exception_fields(value: &CalendarException) -> BTreeMap<String, String> {
    let mut result = BTreeMap::from([
        ("exceptionstarttime".into(), value.original_start.format("%Y%m%dT%H%M%SZ").to_string()),
        ("deleted".into(), u8::from(value.deleted).to_string()),
    ]);
    for (key, field) in [
        ("subject", &value.fields.subject),
        ("location", &value.fields.location),
        ("body", &value.fields.body),
    ] {
        if let Patch::Value(value) = field {
            result.insert(key.into(), value.clone());
        }
    }
    for (key, field) in [("starttime", &value.fields.starts_at), ("endtime", &value.fields.ends_at)]
    {
        if let Patch::Value(Some(value)) = field {
            result.insert(key.into(), value.format("%Y%m%dT%H%M%SZ").to_string());
        }
    }
    if let Patch::Value(value) = value.fields.all_day {
        result.insert("alldayevent".into(), u8::from(value).to_string());
    }
    result
}
