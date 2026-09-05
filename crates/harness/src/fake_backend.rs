use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[path = "fake_backend/calendar_control.rs"]
mod calendar_control;
#[path = "fake_backend/control.rs"]
mod control;
#[path = "fake_backend/fixtures.rs"]
mod fixtures;
#[path = "fake_backend/mail.rs"]
mod mail_fixture;
#[path = "fake_backend/mail_mutations.rs"]
mod mail_mutations;
#[path = "fake_backend/oof.rs"]
mod oof;

use async_trait::async_trait;
use eas_mail_mcp::backend::{
    AccountBackend, BackendAccount, BackendCalendarMutation, BackendCalendarSearch,
    BackendCapabilities, BackendEvent, BackendMail, BackendSync, MailSource, OutgoingMail,
};
use eas_mail_mcp::{AppError, ErrorCode, Result};
use eas_mail_protocol::{
    CandidateAvailability, Folder, FreeBusyStatus, MeetingResponseChoice, RecipientAvailability,
    RecipientResolution, ResolvedRecipient,
};

use self::fixtures::{
    event, event_from_application, folders, personal_event, received_event, recurring_event,
};
use self::mail_fixture::mail;

/// Deterministic high-level backend used by MCP black-box tests.
#[derive(Debug)]
pub struct FakeBackend {
    account: BackendAccount,
    failure: Mutex<Option<ErrorCode>>,
    operation_failure: Mutex<Option<(String, usize, ErrorCode)>>,
    mail_count: usize,
    mail_items: Mutex<BTreeMap<MailSource, BackendMail>>,
    removed_mail: Mutex<std::collections::BTreeSet<MailSource>>,
    include_series: bool,
    operations: Mutex<Vec<String>>,
    calendar_items: Mutex<BTreeMap<String, BackendEvent>>,
    calendar_messages: Mutex<Vec<Vec<u8>>>,
    outgoing_messages: Mutex<Vec<OutgoingMail>>,
    calendar_responses: Mutex<Vec<Option<chrono::DateTime<chrono::Utc>>>>,
    source_resolutions: AtomicUsize,
    created_events: AtomicUsize,
    capabilities: BackendCapabilities,
    oof: Mutex<oof::OofFixture>,
    delay: Duration,
}

#[async_trait]
impl AccountBackend for FakeBackend {
    fn account(&self) -> BackendAccount {
        self.account.clone()
    }

    async fn capabilities(&self) -> Result<BackendCapabilities> {
        self.check().await?;
        Ok(self.capabilities)
    }

    async fn folders(&self) -> Result<Vec<Folder>> {
        self.check().await?;
        Ok(folders())
    }

    async fn get_auto_reply(&self) -> Result<eas_mail_protocol::OofSettings> {
        self.read_auto_reply_fixture().await
    }

    async fn set_auto_reply(&self, settings: &eas_mail_protocol::OofSettings) -> Result<()> {
        self.write_auto_reply_fixture(settings).await
    }

    async fn sync_mail(&self) -> Result<BackendSync> {
        self.check().await?;
        Ok(BackendSync { collections: 1, changes: 1 })
    }

    async fn list_mail(&self, folder_ids: Option<&[String]>) -> Result<Vec<BackendMail>> {
        self.check().await?;
        Ok(if folder_ids.is_none_or(|ids| ids.iter().any(|id| id == "inbox")) {
            (0..self.mail_count)
                .map(|index| {
                    mail(
                        &self.account.account_id,
                        MailSource::Item {
                            folder_id: "inbox".into(),
                            server_id: format!("message-{index}"),
                        },
                    )
                })
                .collect()
        } else {
            Vec::new()
        })
    }

    async fn search_mail(&self, query: &str, _: usize) -> Result<Vec<BackendMail>> {
        self.check().await?;
        let prefix = if query == "meeting-request" { "meeting-request" } else { "long-message" };
        Ok((0..self.mail_count)
            .map(|index| {
                mail(&self.account.account_id, MailSource::LongId(format!("{prefix}-{index}")))
            })
            .collect())
    }

    async fn search_mail_page(
        &self,
        query: &eas_mail_protocol::MailSearchQuery,
        start: usize,
        limit: usize,
    ) -> Result<eas_mail_mcp::backend::BackendMailSearchPage> {
        let items = self.search_mail(&query.text, 1000).await?;
        let total = items.len();
        let items = items.into_iter().skip(start).take(limit).collect::<Vec<_>>();
        let range = (!items.is_empty())
            .then(|| eas_mail_protocol::SearchRange { start, end: start + items.len() - 1 });
        Ok(eas_mail_mcp::backend::BackendMailSearchPage {
            items,
            total: Some(total),
            range,
            server_truncated: false,
        })
    }

    async fn search_people(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<eas_mail_protocol::protocol::DirectoryPage> {
        self.check().await?;
        let values = [
            eas_mail_protocol::protocol::DirectoryPerson {
                name: "Test User".into(),
                email: "user@example.invalid".into(),
            },
            eas_mail_protocol::protocol::DirectoryPerson {
                name: "Test Colleague".into(),
                email: "colleague@example.invalid".into(),
            },
        ];
        let mut items = values
            .into_iter()
            .filter(|person| {
                person.name.to_lowercase().contains(&query.to_lowercase())
                    || person.email.contains(query)
            })
            .collect::<Vec<_>>();
        let total = items.len();
        items.truncate(limit);
        Ok(eas_mail_protocol::protocol::DirectoryPage { items, total })
    }

    async fn fetch_mail(&self, source: &MailSource, _: usize) -> Result<BackendMail> {
        self.check().await?;
        self.stored_mail(source)
    }

    async fn resolve_mail_source(&self, source: &MailSource) -> Result<BackendMail> {
        self.check().await?;
        let source = match source {
            MailSource::LongId(id) if id.starts_with("long-message-") => MailSource::Item {
                folder_id: "inbox".into(),
                server_id: id.replacen("long-message-", "message-", 1),
            },
            other => other.clone(),
        };
        self.stored_mail(&source)
    }

    async fn move_mail(&self, source: &MailSource, destination: &str) -> Result<MailSource> {
        self.fake_move(source, destination).await
    }

    async fn set_mail_flag(&self, source: &MailSource, status: u8) -> Result<()> {
        self.fake_flag(source, status).await
    }

    async fn set_mail_categories(&self, source: &MailSource, categories: &[String]) -> Result<()> {
        self.fake_categories(source, categories).await
    }

    async fn fetch_attachment(&self, _: &str) -> Result<Vec<u8>> {
        self.check().await?;
        Ok(b"attachment payload".to_vec())
    }

    async fn calendar_availability(
        &self,
        participants: &[String],
        starts_at: chrono::DateTime<chrono::Utc>,
        ends_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<RecipientAvailability>> {
        self.check().await?;
        let milliseconds = ends_at.signed_duration_since(starts_at).num_milliseconds();
        let slots = milliseconds
            .saturating_add(1_799_999)
            .checked_div(1_800_000)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| failure(ErrorCode::ProtocolError))?;
        Ok(participants
            .iter()
            .map(|input| RecipientAvailability {
                input: input.clone(),
                resolution: RecipientResolution::Resolved,
                total_candidates: 1,
                candidates: vec![ResolvedRecipient {
                    recipient_type: 1,
                    display_name: "Test User".into(),
                    email: input.clone(),
                    availability: CandidateAvailability::Slots(vec![FreeBusyStatus::Free; slots]),
                }],
            })
            .collect())
    }

    async fn search_calendar(&self, query: &str, limit: usize) -> Result<BackendCalendarSearch> {
        self.check().await?;
        let events = (limit > 0)
            .then(|| {
                if query == "received" {
                    received_event(&self.account.account_id)
                } else if query == "personal" {
                    personal_event(&self.account.account_id)
                } else if query == "recurring" {
                    recurring_event(&self.account.account_id)
                } else {
                    event(&self.account.account_id)
                }
            })
            .into_iter()
            .collect();
        Ok(BackendCalendarSearch { events, total: 1 })
    }

    async fn scan_calendar_metadata(&self) -> Result<BackendCalendarSearch> {
        self.check().await?;
        let mut events = vec![event(&self.account.account_id)];
        if self.include_series {
            events.push(recurring_event(&self.account.account_id));
        }
        events.extend(
            self.calendar_items
                .lock()
                .map_err(|_| failure(ErrorCode::StorageError))?
                .iter()
                .filter(|(key, _)| key.starts_with("event-created"))
                .map(|(_, value)| value.clone()),
        );
        Ok(BackendCalendarSearch { total: events.len(), events })
    }

    async fn fetch_calendar(&self, source: &BackendEvent, _: usize) -> Result<BackendEvent> {
        self.check().await?;
        self.calendar_item(source)
    }

    async fn resolve_calendar_source(&self, source: &BackendEvent) -> Result<BackendEvent> {
        self.source_resolutions.fetch_add(1, Ordering::Relaxed);
        self.check().await?;
        self.calendar_item(source)
    }

    async fn create_calendar_item(
        &self,
        _: &str,
        item: &BackendCalendarMutation,
    ) -> Result<BackendEvent> {
        self.check_operation("calendar_create_item").await?;
        self.record("calendar_create_item")?;
        let mut event = event_from_application(&self.account.account_id, &item.application);
        let index = self.created_events.fetch_add(1, Ordering::Relaxed);
        if index > 0 {
            event.server_id = Some(format!("event-created-{index}"));
        }
        self.store_calendar_item(event.clone())?;
        Ok(event)
    }

    async fn update_calendar_item(
        &self,
        source: &BackendEvent,
        item: &BackendCalendarMutation,
    ) -> Result<BackendEvent> {
        self.check_operation("calendar_update_item").await?;
        self.record("calendar_update_item")?;
        let mut output = event_from_application(&self.account.account_id, &item.application);
        output.collection_id.clone_from(&source.collection_id);
        output.server_id.clone_from(&source.server_id);
        if output.server_id.as_deref().is_some_and(|id| id.starts_with("event-created")) {
            self.store_calendar_item(output.clone())?;
        }
        Ok(output)
    }

    async fn delete_calendar_item(&self, source: &BackendEvent) -> Result<()> {
        self.check_operation("calendar_delete_item").await?;
        self.record("calendar_delete_item")?;
        self.remove_calendar_item(source)
    }

    async fn respond_calendar_item(
        &self,
        source: &BackendEvent,
        _: MeetingResponseChoice,
    ) -> Result<Option<String>> {
        self.check_operation("calendar_respond_item").await?;
        self.record("calendar_respond_item")?;
        self.calendar_responses
            .lock()
            .map_err(|_| failure(ErrorCode::StorageError))?
            .push(source.occurrence_start);
        Ok(Some("responded-event".into()))
    }

    async fn respond_meeting_request(
        &self,
        _: &MailSource,
        _: MeetingResponseChoice,
    ) -> Result<Option<String>> {
        self.check_operation("calendar_respond_request").await?;
        self.record("calendar_respond_request")?;
        Ok(Some("responded-event".into()))
    }

    async fn send_calendar_message(&self, _: &str, mime: Vec<u8>) -> Result<()> {
        self.check_operation("calendar_send").await?;
        self.calendar_messages.lock().map_err(|_| failure(ErrorCode::StorageError))?.push(mime);
        self.record("calendar_send")
    }

    async fn mark_read(&self, source: &MailSource, is_read: bool) -> Result<()> {
        self.fake_read(source, is_read).await
    }

    async fn send(&self, _: &str, message: &OutgoingMail) -> Result<()> {
        self.check_operation("mail_send").await?;
        self.outgoing_messages
            .lock()
            .map_err(|_| failure(ErrorCode::StorageError))?
            .push(message.clone());
        self.record("mail_send")
    }

    async fn reply(&self, _: &str, _: &MailSource, message: &OutgoingMail) -> Result<()> {
        self.check_operation("mail_reply").await?;
        self.outgoing_messages
            .lock()
            .map_err(|_| failure(ErrorCode::StorageError))?
            .push(message.clone());
        self.record("mail_reply")
    }

    async fn forward(&self, _: &str, _: &MailSource, message: &OutgoingMail) -> Result<()> {
        self.check_operation("mail_forward").await?;
        self.outgoing_messages
            .lock()
            .map_err(|_| failure(ErrorCode::StorageError))?
            .push(message.clone());
        self.record("mail_forward")
    }
}

fn failure(code: ErrorCode) -> AppError {
    let error = AppError::new(code, "scripted backend is unavailable");
    if code == ErrorCode::NetworkUnreachable { error.retryable() } else { error }
}
