use eas_mail_protocol::{CalendarApplication, CalendarAttendee};
use icalendar::{Attendee, Calendar, Component as _, Event, Property, Role};

use crate::Result;
use crate::model::CalendarResponseChoice;

pub(super) fn calendar(
    sender: &str,
    recipients: &[CalendarAttendee],
    item: &CalendarApplication,
    response: CalendarResponseChoice,
    comment: &str,
) -> Result<String> {
    let organizer =
        recipients.first().ok_or_else(|| super::validation("calendar reply has no organizer"))?;
    // RFC 5546 section 3.2.3: a reply needs identity and attendance, not a copy of
    // the organizer's full event. In particular, a series reply must not echo
    // sibling exceptions as if the attendee had responded to each separately.
    let mut event = Event::new();
    event.uid(&item.uid).timestamp(item.dt_stamp);
    event.append_property(Property::new("ORGANIZER", format!("mailto:{}", organizer.email)));
    event.attendee(
        Attendee::new(format!("mailto:{sender}"))
            .role(Role::ReqParticipant)
            .partstat(super::part_stat(response))
            .rsvp(false),
    );
    if !comment.is_empty() {
        event.append_property(Property::new("COMMENT", comment));
    }
    if let Some(original) = item.properties.instance_start {
        let property = if item.properties.instance_all_day.unwrap_or(item.all_day) {
            Property::new(
                "RECURRENCE-ID",
                crate::runtime::calendar_series::zone(item)?
                    .to_local(original)?
                    .format("%Y%m%d")
                    .to_string(),
            )
            .add_parameter("VALUE", "DATE")
            .done()
        } else {
            Property::new("RECURRENCE-ID", original.format("%Y%m%dT%H%M%SZ").to_string())
        };
        event.append_property(property);
    }
    let mut calendar = Calendar::empty();
    calendar
        .append_property(Property::new("PRODID", "-//EAS Mail MCP//EN"))
        .append_property(Property::new("VERSION", "2.0"))
        .append_property(Property::new("CALSCALE", "GREGORIAN"))
        .append_property(Property::new("METHOD", "REPLY"))
        .push(event);
    Ok(calendar.to_string())
}
