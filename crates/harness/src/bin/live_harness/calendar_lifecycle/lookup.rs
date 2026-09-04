use std::io::{self, Write as _};
use std::time::Duration;

use eas_mail_mcp::{
    CalendarEvent, CalendarEventType, CalendarGetInput, CalendarMailKind, CalendarSearchInput,
    MailGetInput, MailSearchInput, MailSummary, Runtime,
};

use super::super::checks::required;

const DELIVERY_ATTEMPTS: usize = 360;
const DELIVERY_DELAY: Duration = Duration::from_secs(5);
const PROGRESS_INTERVAL: usize = 12;
const DELIVERY_WINDOW_MINUTES: usize = DELIVERY_ATTEMPTS / PROGRESS_INTERVAL;
const MEETING_SEARCH_QUERY: &str = "EAS Mail MCP meeting";
const PERSONAL_SEARCH_QUERY: &str = "EAS Mail MCP personal";

#[derive(Debug, Default)]
struct MeetingMailObservation {
    matching: usize,
    fetched: usize,
    actionable: usize,
    request: usize,
    update: usize,
    cancellation: usize,
    response: usize,
    other: usize,
    unclassified: usize,
    fetch_error: bool,
}

impl MeetingMailObservation {
    fn record(&mut self, summary: &MailSummary) {
        self.fetched += 1;
        self.actionable += usize::from(summary.can_respond);
        match summary.calendar_message {
            Some(CalendarMailKind::Request) => self.request += 1,
            Some(CalendarMailKind::Update) => self.update += 1,
            Some(CalendarMailKind::Cancellation) => self.cancellation += 1,
            Some(CalendarMailKind::Response) => self.response += 1,
            Some(CalendarMailKind::Other) => self.other += 1,
            None => self.unclassified += 1,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ExpectedEvent {
    Personal,
    Organizer,
    Attendee,
}

pub async fn wait_for_event(
    runtime: &Runtime,
    account_id: &str,
    token: &str,
    uid: Option<&str>,
    expected: ExpectedEvent,
) -> anyhow::Result<CalendarEvent> {
    for attempt in 0..DELIVERY_ATTEMPTS {
        if let Some(event) = find_event(runtime, account_id, token, uid, expected).await? {
            return Ok(event);
        }
        report_wait("Calendar item", attempt)?;
        tokio::time::sleep(DELIVERY_DELAY).await;
    }
    anyhow::bail!(
        "Calendar item did not reach the expected account within {DELIVERY_WINDOW_MINUTES} minutes"
    )
}

pub async fn find_event(
    runtime: &Runtime,
    account_id: &str,
    token: &str,
    uid: Option<&str>,
    expected: ExpectedEvent,
) -> anyhow::Result<Option<CalendarEvent>> {
    let search = required(
        runtime
            .calendar_search(CalendarSearchInput {
                query: Some(event_search_query(expected).to_owned()),
                date_from: None,
                date_to: None,
                time_zone: None,
                account_ids: Some(vec![account_id.to_owned()]),
                limit: Some(20),
            })
            .await,
        "calendar_search lifecycle item",
    )?;
    for summary in search.items.into_iter().filter(|event| event.subject.contains(token)) {
        let response = runtime
            .calendar_get(CalendarGetInput {
                event_ref: summary.event_ref,
                body_limit: Some(12_000),
            })
            .await;
        let Some(event) = response.data else {
            continue;
        };
        if uid.is_none_or(|value| event.uid == value) && event_matches(&event, expected) {
            return Ok(Some(event));
        }
    }
    Ok(None)
}

pub async fn wait_for_meeting_mail(
    runtime: &Runtime,
    account_id: &str,
    token: &str,
    expected: CalendarMailKind,
) -> anyhow::Result<MailSummary> {
    let mut observed = MeetingMailObservation::default();
    for attempt in 0..DELIVERY_ATTEMPTS {
        let result = search_mail(runtime, account_id).await?;
        let mut current = MeetingMailObservation::default();
        for summary in result.items.into_iter().filter(|mail| mail.subject.contains(token)) {
            current.matching += 1;
            let detail = runtime
                .mail_get(MailGetInput { mail_ref: summary.mail_ref, body_limit: Some(12_000) })
                .await;
            if let Some(detail) = detail.data {
                current.record(&detail.summary);
                if detail.summary.calendar_message == Some(expected)
                    && (!requires_action(expected) || detail.summary.can_respond)
                {
                    return Ok(detail.summary);
                }
            } else {
                current.fetch_error = true;
            }
        }
        observed = current;
        report_wait("Calendar mail", attempt)?;
        tokio::time::sleep(DELIVERY_DELAY).await;
    }
    anyhow::bail!(
        "Expected Calendar mail did not reach the mailbox within \
         {DELIVERY_WINDOW_MINUTES} minutes: {observed:?}"
    )
}

const fn requires_action(kind: CalendarMailKind) -> bool {
    matches!(kind, CalendarMailKind::Request | CalendarMailKind::Update)
}

fn report_wait(stage: &str, attempt: usize) -> anyhow::Result<()> {
    let completed = attempt + 1;
    if completed.is_multiple_of(PROGRESS_INTERVAL) {
        writeln!(
            io::stderr(),
            "{stage} delivery is still pending after {} minute(s)",
            completed / PROGRESS_INTERVAL
        )?;
    }
    Ok(())
}

async fn search_mail(
    runtime: &Runtime,
    account_id: &str,
) -> anyhow::Result<eas_mail_mcp::MailPage> {
    required(
        runtime
            .mail_search(MailSearchInput {
                filters: Default::default(),
                query: MEETING_SEARCH_QUERY.to_owned(),
                account_ids: Some(vec![account_id.to_owned()]),
                cursor: None,
                limit: Some(100),
            })
            .await,
        "mail_search meeting notification",
    )
}

fn event_search_query(expected: ExpectedEvent) -> &'static str {
    match expected {
        ExpectedEvent::Personal => PERSONAL_SEARCH_QUERY,
        ExpectedEvent::Organizer | ExpectedEvent::Attendee => MEETING_SEARCH_QUERY,
    }
}

fn event_matches(event: &CalendarEvent, expected: ExpectedEvent) -> bool {
    match expected {
        ExpectedEvent::Personal => {
            event.event_type == CalendarEventType::Personal && event.can_update && event.can_delete
        }
        ExpectedEvent::Organizer => {
            event.event_type == CalendarEventType::OrganizerMeeting
                && event.can_update
                && event.can_cancel
        }
        ExpectedEvent::Attendee => {
            event.event_type == CalendarEventType::AttendeeMeeting && event.can_respond
        }
    }
}
