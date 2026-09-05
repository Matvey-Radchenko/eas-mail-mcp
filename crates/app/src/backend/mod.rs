//! Account-scoped mailbox boundary and its production EAS implementation.

mod eas_mailbox;
mod model;
mod unavailable;
pub(crate) use unavailable::UnavailableBackend;

use async_trait::async_trait;

use crate::Result;

pub use eas_mailbox::EasMailbox;
pub(crate) use eas_mailbox::VerificationStage;
pub use model::{
    BackendAccount, BackendCalendarMutation, BackendCalendarSearch, BackendCapabilities,
    BackendEvent, BackendMail, BackendMailSearchPage, BackendSync, MailSource, OutgoingMail,
};

/// Network-backed operations for exactly one configured account.
#[async_trait]
pub trait AccountBackend: Send + Sync {
    /// Returns safe account metadata.
    fn account(&self) -> BackendAccount;

    /// Known local account setup failure; absence does not imply verified connectivity.
    fn configuration_error(&self) -> Option<crate::ErrorEnvelope> {
        None
    }

    /// Negotiates and returns optional server capabilities.
    async fn capabilities(&self) -> Result<BackendCapabilities>;

    /// Reads automatic-reply settings without changing them.
    async fn get_auto_reply(&self) -> Result<eas_mail_protocol::OofSettings> {
        Err(crate::AppError::new(
            crate::ErrorCode::FeatureUnavailable,
            "automatic replies are unavailable",
        ))
    }

    /// Applies one Settings/Oof Set; the caller verifies it with a separate read.
    async fn set_auto_reply(&self, _settings: &eas_mail_protocol::OofSettings) -> Result<()> {
        Err(crate::AppError::new(
            crate::ErrorCode::FeatureUnavailable,
            "automatic replies are unavailable",
        ))
    }

    /// Refreshes and returns the managed folder hierarchy.
    async fn folders(&self) -> Result<Vec<eas_mail_protocol::Folder>>;

    /// Refreshes all mail collections into process-local memory.
    async fn sync_mail(&self) -> Result<BackendSync>;

    /// Performs a fresh mail synchronization and returns the resulting snapshot.
    async fn list_mail(&self, folder_ids: Option<&[String]>) -> Result<Vec<BackendMail>>;

    /// Performs EAS Search and returns server results.
    async fn search_mail(&self, query: &str, limit: usize) -> Result<Vec<BackendMail>>;

    /// Retrieves one bounded EAS Search page without synchronizing mail collections.
    async fn search_mail_page(
        &self,
        query: &eas_mail_protocol::MailSearchQuery,
        start: usize,
        limit: usize,
    ) -> Result<BackendMailSearchPage>;

    /// Resolves one mutable mail locator by ItemOperations, with no Sync fallback.
    async fn resolve_mail_source(&self, source: &MailSource) -> Result<BackendMail> {
        let mail = self.fetch_mail(source, 1).await?;
        if matches!(mail.source, MailSource::Item { .. }) {
            Ok(mail)
        } else {
            Err(crate::AppError::new(
                crate::ErrorCode::FeatureUnavailable,
                "Exchange did not provide mutable identifiers for this message",
            ))
        }
    }

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

    /// Checks process-local property-write prerequisites before an operation UUID is claimed.
    /// This check must not synchronize or issue any other network request.
    async fn check_mail_property_ready(&self, _source: &MailSource) -> Result<()> {
        Ok(())
    }

    /// Changes one message's read state.
    async fn mark_read(&self, source: &MailSource, is_read: bool) -> Result<()>;

    /// Moves one message within this account and returns its resulting locator.
    async fn move_mail(&self, _source: &MailSource, _destination: &str) -> Result<MailSource> {
        Err(crate::AppError::new(crate::ErrorCode::FeatureUnavailable, "mail move is unavailable"))
    }

    /// Changes flag status while preserving supported existing flag parameters.
    async fn set_mail_flag(&self, _source: &MailSource, _status: u8) -> Result<()> {
        Err(crate::AppError::new(
            crate::ErrorCode::FeatureUnavailable,
            "mail flags are unavailable",
        ))
    }

    /// Replaces the message category set; an empty set clears it.
    async fn set_mail_categories(
        &self,
        _source: &MailSource,
        _categories: &[String],
    ) -> Result<()> {
        Err(crate::AppError::new(
            crate::ErrorCode::FeatureUnavailable,
            "mail categories are unavailable",
        ))
    }

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
