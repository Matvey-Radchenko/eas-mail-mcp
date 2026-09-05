use chrono::Utc;
use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{CalendarApplication, Command, EasClient, MeetingResponseChoice, Result};

use super::boundary::{collection, sync};

#[derive(Clone, Copy)]
pub enum Operation {
    Add,
    Change,
    Delete,
    Meeting,
    Instance,
    LongId,
}

pub const CALENDAR: [Operation; 3] = [Operation::Add, Operation::Change, Operation::Delete];
pub const MEETINGS: [Operation; 3] = [Operation::Meeting, Operation::Instance, Operation::LongId];

impl Operation {
    pub fn command(self) -> Command {
        match self {
            Self::Add | Self::Change | Self::Delete => Command::Sync,
            _ => Command::MeetingResponse,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Add => "Add",
            Self::Change => "Change",
            Self::Delete => "Delete",
            _ => "Result",
        }
    }
    pub fn id(self) -> &'static str {
        if matches!(self, Self::Add) { "client" } else { "item" }
    }
    pub async fn run(self, client: &EasClient) -> Result<u16> {
        let now = Utc::now();
        let item = CalendarApplication {
            properties: Default::default(),
            time_zone: "AAAA".into(),
            uid: "uid".into(),
            dt_stamp: now,
            starts_at: now,
            ends_at: now + chrono::Duration::minutes(30),
            all_day: false,
            subject: "Synthetic".into(),
            body: String::new(),
            location: String::new(),
            reminder_minutes: None,
            busy_status: 2,
            meeting_status: 0,
            response_requested: false,
            attendees: Vec::new(),
        };
        match self {
            Self::Add => client
                .calendar_add(7, "calendar", "key", "client", &item)
                .await
                .map(|value| value.status),
            Self::Change => client
                .calendar_change(7, "calendar", "item", "key", &item)
                .await
                .map(|value| value.status),
            Self::Delete => {
                client.calendar_delete(7, "calendar", "item", "key").await.map(|value| value.status)
            }
            Self::Meeting => client
                .meeting_response(7, "calendar", "item", MeetingResponseChoice::Accept)
                .await
                .map(|value| value.status),
            Self::Instance => client
                .meeting_response_instance(
                    7,
                    "calendar",
                    "item",
                    MeetingResponseChoice::Tentative,
                    Some(now),
                )
                .await
                .map(|value| value.status),
            Self::LongId => client
                .meeting_response_long_id(7, "item", MeetingResponseChoice::Decline)
                .await
                .map(|value| value.status),
        }
    }
}

pub fn item(command: &str, status: &str, id: Option<&str>) -> Element {
    let mut item = Element::new("AirSync", command);
    item.push(Element::text("AirSync", "Status", status));
    if let Some(id) = id {
        item.push(Element::text(
            "AirSync",
            if command == "Add" { "ClientId" } else { "ServerId" },
            id,
        ));
    }
    if command == "Add" {
        item.push(Element::text("AirSync", "ServerId", "new-item"));
    }
    item
}

pub fn accepted(items: Option<Vec<Element>>) -> anyhow::Result<Vec<u8>> {
    sync(Some(collection(
        &[("CollectionId", "calendar"), ("Status", "1"), ("SyncKey", "next")],
        items,
    )))
}

pub fn status_at(operation: Operation, value: &str, level: &str) -> anyhow::Result<Vec<u8>> {
    let mut root = Element::new("AirSync", "Sync");
    if level == "root" {
        root.push(Element::text("AirSync", "Status", value));
    }
    let mut collections = Element::new("AirSync", "Collections");
    collections.push(collection(
        &[
            ("CollectionId", "calendar"),
            ("SyncKey", "next"),
            ("Status", if level == "collection" { value } else { "1" }),
        ],
        Some(vec![item(
            operation.name(),
            if level == "item" { value } else { "1" },
            Some(operation.id()),
        )]),
    ));
    root.push(collections);
    Ok(encode(&root)?)
}

pub fn meeting(operation: Operation, status: &str, echo: bool) -> anyhow::Result<Vec<u8>> {
    let mut result = Element::new("MeetingResponse", "Result");
    result.push(Element::text("MeetingResponse", "Status", status));
    if echo {
        result.push(meeting_id(operation, "item"));
    }
    meeting_results(vec![result])
}

fn meeting_id(operation: Operation, id: &str) -> Element {
    if matches!(operation, Operation::LongId) {
        Element::text("Search", "LongId", id)
    } else {
        Element::text("MeetingResponse", "RequestId", id)
    }
}

fn meeting_results(results: Vec<Element>) -> anyhow::Result<Vec<u8>> {
    let mut root = Element::new("MeetingResponse", "MeetingResponse");
    for result in results {
        root.push(result);
    }
    Ok(encode(&root)?)
}

pub fn malformed_meeting(operation: Operation) -> anyhow::Result<Vec<Vec<u8>>> {
    let mut good = Element::new("MeetingResponse", "Result");
    good.push(Element::text("MeetingResponse", "Status", "1"));
    let mut duplicate = good.clone();
    duplicate.push(Element::text("MeetingResponse", "Status", "1"));
    let mut mismatch = good.clone();
    mismatch.push(meeting_id(operation, "different"));
    let mut wrong_kind = good.clone();
    wrong_kind.push(meeting_id(
        if matches!(operation, Operation::LongId) { Operation::Meeting } else { Operation::LongId },
        "item",
    ));
    let mut nested = Element::new("MeetingResponse", "Request");
    nested.push(good.clone());
    let mut malformed = Element::new("MeetingResponse", "Result");
    let mut status = Element::new("MeetingResponse", "Status");
    status.push(Element::text("MeetingResponse", "Status", "1"));
    malformed.push(status);
    let mut truncated = meeting(operation, "1", true)?;
    truncated.pop();
    Ok(vec![
        truncated,
        meeting_results(Vec::new())?,
        meeting_results(vec![good.clone(), good])?,
        meeting_results(vec![duplicate])?,
        meeting_results(vec![mismatch])?,
        meeting_results(vec![wrong_kind])?,
        meeting_results(vec![nested])?,
        meeting_results(vec![Element::new("MeetingResponse", "Result")])?,
        meeting_results(vec![malformed])?,
    ])
}
