use crate::{CalendarApplication, CalendarFields, Patch};

impl From<&CalendarApplication> for CalendarFields {
    fn from(item: &CalendarApplication) -> Self {
        Self {
            properties: Some(item.properties.clone()),
            subject: Patch::Value(item.subject.clone()),
            body: Patch::Value(item.body.clone()),
            body_truncated: Patch::Value(false),
            starts_at: Patch::Value(Some(item.starts_at)),
            ends_at: Patch::Value(Some(item.ends_at)),
            all_day: Patch::Value(item.all_day),
            location: Patch::Value(item.location.clone()),
            attendees: Patch::Value(item.attendees.clone()),
            reminder_minutes: Patch::Value(item.reminder_minutes),
            recurrence: Patch::Value(
                item.properties
                    .recurrence
                    .as_ref()
                    .map(crate::CalendarRecurrence::to_fields)
                    .unwrap_or_default(),
            ),
            exceptions: Patch::Value(
                item.properties.exceptions.iter().map(crate::protocol::exception_fields).collect(),
            ),
            meeting_status: Patch::Value(item.meeting_status),
            uid: Patch::Value(item.uid.clone()),
            dt_stamp: Patch::Value(Some(item.dt_stamp)),
            time_zone: Patch::Value(item.time_zone.clone()),
            busy_status: Patch::Value(item.busy_status),
            response_requested: Patch::Value(item.response_requested),
            ..Self::default()
        }
    }
}
