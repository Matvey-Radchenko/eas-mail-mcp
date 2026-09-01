//! Account-scoped mailbox boundary and its production EAS implementation.

mod eas_mailbox;
mod model;

use async_trait::async_trait;

use crate::Result;

pub use eas_mailbox::EasMailbox;
pub(crate) use eas_mailbox::VerificationStage;
pub use model::{
    BackendAccount, BackendCalendarMutation, BackendCalendarSearch, BackendCapabilities,
    BackendEvent, BackendMail, BackendSync, MailSource, OutgoingMail,
};

/// Network-backed operations for exactly one configured account.
#[async_trait]
pub trait AccountBackend: Send + Sync {
    /// Returns safe account metadata.
    fn account(&self) -> BackendAccount;

    /// Negotiates and returns optional server capabilities.
    async fn capabilities(&self) -> Result<BackendCapabilities>;

    /// Refreshes and returns the managed folder hierarchy.
    async fn folders(&self) -> Result<Vec<eas_mail_protocol::Folder>>;

    /// Refreshes all mail collections into process-local memory.
    async fn sync_mail(&self) -> Result<BackendSync>;

    /// Performs a fresh mail synchronization and returns the resulting snapshot.
    async fn list_mail(&self, folder_ids: Option<&[String]>) -> Result<Vec<BackendMail>>;

    /// Performs EAS Search and returns server results.
    async fn search_mail(&self, query: &str, limit: usize) -> Result<Vec<BackendMail>>;

    /// Searches only the account's global address list.
    async fn search_people(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<eas_mail_protocol::protocol::DirectoryPage>;

    /// Fetches a full message body from Exchange.
    async fn fetch_mail(&self, source: &MailSource, body_limit: usize) -> Result<BackendMail>;

    /// Downloads attachment bytes from Exchange.
    async fn fetch_attachment(&self, file_reference: &str) -> Result<Vec<u8>>;

    /// Resolves directory recipients and returns one free/busy range.
    async fn calendar_availability(
        &self,
        participants: &[String],
        starts_at: chrono::DateTime<chrono::Utc>,
        ends_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<eas_mail_protocol::RecipientAvailability>>;

    /// Performs bounded server-side Calendar Search.
    async fn search_calendar(&self, query: &str, limit: usize) -> Result<BackendCalendarSearch>;

    /// Reads a fresh metadata-only Calendar snapshot for local agenda filtering.
    async fn scan_calendar_metadata(&self) -> Result<BackendCalendarSearch>;

    /// Fetches one full Calendar item from a Search LongId.
    async fn fetch_calendar(
        &self,
        source: &BackendEvent,
        body_limit: usize,
    ) -> Result<BackendEvent>;

    /// Resolves collection/server identifiers for one mutable Calendar source.
    async fn resolve_calendar_source(&self, source: &BackendEvent) -> Result<BackendEvent>;

    /// Adds one non-recurring Calendar item.
    async fn create_calendar_item(
        &self,
        client_id: &str,
        item: &BackendCalendarMutation,
    ) -> Result<BackendEvent>;

    /// Replaces one non-recurring Calendar item.
    async fn update_calendar_item(
        &self,
        source: &BackendEvent,
        item: &BackendCalendarMutation,
    ) -> Result<BackendEvent>;

    /// Deletes one Calendar item.
    async fn delete_calendar_item(&self, source: &BackendEvent) -> Result<()>;

    /// Applies MeetingResponse and returns the resulting Calendar server ID.
    async fn respond_calendar_item(
        &self,
        source: &BackendEvent,
        response: eas_mail_protocol::MeetingResponseChoice,
    ) -> Result<Option<String>>;

    /// Applies MeetingResponse to one Inbox meeting request.
    async fn respond_meeting_request(
        &self,
        source: &MailSource,
        response: eas_mail_protocol::MeetingResponseChoice,
    ) -> Result<Option<String>>;

    /// Sends a prebuilt calendar MIME message through EAS SendMail.
    async fn send_calendar_message(&self, client_id: &str, mime: Vec<u8>) -> Result<()>;

    /// Changes one message's read state.
    async fn mark_read(&self, source: &MailSource, is_read: bool) -> Result<()>;

    /// Sends a new message.
    async fn send(&self, client_id: &str, message: &OutgoingMail) -> Result<()>;

    /// Replies to an existing message.
    async fn reply(
        &self,
        client_id: &str,
        source: &MailSource,
        message: &OutgoingMail,
    ) -> Result<()>;

    /// Forwards an existing message.
    async fn forward(
        &self,
        client_id: &str,
        source: &MailSource,
        message: &OutgoingMail,
    ) -> Result<()>;
}
