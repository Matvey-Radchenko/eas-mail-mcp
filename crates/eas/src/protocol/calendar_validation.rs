use crate::wbxml::Element;

use super::tree::{direct_text, parse_datetime};

pub(super) fn fields_supported(parent: &Element) -> bool {
    let numbers = [
        ("AllDayEvent", 1),
        ("Deleted", 1),
        ("ResponseRequested", 1),
        ("BusyStatus", 4),
        ("MeetingStatus", 7),
        ("ResponseType", 5),
        ("Reminder", u32::MAX),
    ];
    for (name, maximum) in numbers {
        if direct_text(parent, "Calendar", name).is_some_and(|value| {
            !(value.parse::<u32>().is_ok_and(|number| number <= maximum)
                || name == "Reminder" && value.is_empty())
        }) {
            return false;
        }
    }
    for name in ["StartTime", "EndTime", "DtStamp", "ExceptionStartTime"] {
        if let Some(value) = direct_text(parent, "Calendar", name)
            && parse_datetime(Some(value)).is_none()
        {
            return false;
        }
    }
    for name in ["AppointmentReplyTime", "OnlineMeetingConfLink", "OnlineMeetingExternalLink"] {
        if direct_text(parent, "Calendar", name).is_some_and(|value| !value.is_empty()) {
            return false;
        }
    }
    parent
        .child("Calendar", "Attendees")
        .is_none_or(|container| container.children().all(attendee_supported))
        && parent.child("AirSyncBase", "Body").is_none_or(body_supported)
}

fn body_supported(body: &Element) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    body.children().all(|child| {
        child.namespace == "AirSyncBase"
            && matches!(
                child.name.as_str(),
                "Type" | "Data" | "EstimatedDataSize" | "Truncated" | "Preview"
            )
            && seen.insert(&child.name)
    }) && direct_text(body, "AirSyncBase", "Type")
        .is_none_or(|value| matches!(value.as_str(), "1" | "2"))
        && direct_text(body, "AirSyncBase", "Truncated").is_none_or(|value| value == "0")
}

fn attendee_supported(value: &Element) -> bool {
    if value.namespace != "Calendar" || value.name != "Attendee" {
        return false;
    }
    let mut fields = std::collections::BTreeSet::new();
    for child in value.children() {
        if child.namespace != "Calendar"
            || !matches!(child.name.as_str(), "Email" | "Name" | "AttendeeType" | "AttendeeStatus")
            || !fields.insert(&child.name)
        {
            return false;
        }
    }
    if !direct_text(value, "Calendar", "Email").is_some_and(|email| {
        !email.chars().any(char::is_control)
            && email.split_once('@').is_some_and(|(a, b)| !a.is_empty() && !b.is_empty())
    }) {
        return false;
    }
    for (name, minimum, maximum) in [("AttendeeType", 1, 3), ("AttendeeStatus", 0, 5)] {
        if direct_text(value, "Calendar", name).is_some_and(|value| {
            !value.parse::<u8>().is_ok_and(|number| (minimum..=maximum).contains(&number))
        }) {
            return false;
        }
    }
    true
}
