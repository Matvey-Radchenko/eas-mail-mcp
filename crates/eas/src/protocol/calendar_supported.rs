use crate::wbxml::Element;

pub(super) fn properties() -> Element {
    // MS-ASCMD 2.2.3.179: advertise writable fields at SyncKey=0. Omitted
    // server-managed links and reply metadata are ghosted and survive Change.
    let mut supported = Element::new("AirSync", "Supported");
    for name in [
        "DtStamp",
        "Categories",
        "Sensitivity",
        "BusyStatus",
        "UID",
        "TimeZone",
        "StartTime",
        "Subject",
        "Location",
        "EndTime",
        "Recurrence",
        "AllDayEvent",
        "Reminder",
        "Exceptions",
        "Attendees",
        "MeetingStatus",
        "ResponseRequested",
    ] {
        supported.push(Element::new("Calendar", name));
    }
    supported
}
