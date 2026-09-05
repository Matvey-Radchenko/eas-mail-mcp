use super::{
    AccountBackend, BackendAccount, BackendCalendarMutation, BackendCalendarSearch,
    BackendCapabilities, BackendEvent, BackendMail, BackendMailSearchPage, BackendSync, MailSource,
    OutgoingMail,
};
use crate::Result;
use crate::{AppError, ErrorEnvelope};
use async_trait::async_trait;

/// Retains an unavailable configured account so other accounts remain usable.
pub(crate) struct UnavailableBackend {
    account: BackendAccount,
    error: ErrorEnvelope,
}

impl UnavailableBackend {
    pub(crate) fn new(account: BackendAccount, error: AppError) -> Self {
        Self { account, error: error.envelope }
    }
    fn error<T>(&self) -> Result<T> {
        Err(AppError { envelope: self.error.clone() })
    }
}

#[async_trait]
impl AccountBackend for UnavailableBackend {
    fn account(&self) -> BackendAccount {
        self.account.clone()
    }
    fn configuration_error(&self) -> Option<ErrorEnvelope> {
        Some(self.error.clone())
    }
    async fn capabilities(&self) -> Result<BackendCapabilities> {
        self.error()
    }
    async fn get_auto_reply(&self) -> Result<eas_mail_protocol::OofSettings> {
        self.error()
    }
    async fn set_auto_reply(&self, _: &eas_mail_protocol::OofSettings) -> Result<()> {
        self.error()
    }
    async fn folders(&self) -> Result<Vec<eas_mail_protocol::Folder>> {
        self.error()
    }
    async fn sync_mail(&self) -> Result<BackendSync> {
        self.error()
    }
    async fn list_mail(&self, _: Option<&[String]>) -> Result<Vec<BackendMail>> {
        self.error()
    }
    async fn search_mail(&self, _: &str, _: usize) -> Result<Vec<BackendMail>> {
        self.error()
    }
    async fn search_mail_page(
        &self,
        _: &eas_mail_protocol::MailSearchQuery,
        _: usize,
        _: usize,
    ) -> Result<BackendMailSearchPage> {
        self.error()
    }
    async fn search_people(
        &self,
        _: &str,
        _: usize,
    ) -> Result<eas_mail_protocol::protocol::DirectoryPage> {
        self.error()
    }
    async fn fetch_mail(&self, _: &MailSource, _: usize) -> Result<BackendMail> {
        self.error()
    }
    async fn fetch_attachment(&self, _: &str) -> Result<Vec<u8>> {
        self.error()
    }
    async fn calendar_availability(
        &self,
        _: &[String],
        _: chrono::DateTime<chrono::Utc>,
        _: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<eas_mail_protocol::RecipientAvailability>> {
        self.error()
    }
    async fn search_calendar(&self, _: &str, _: usize) -> Result<BackendCalendarSearch> {
        self.error()
    }
    async fn scan_calendar_metadata(&self) -> Result<BackendCalendarSearch> {
        self.error()
    }
    async fn fetch_calendar(&self, _: &BackendEvent, _: usize) -> Result<BackendEvent> {
        self.error()
    }
    async fn resolve_calendar_source(&self, _: &BackendEvent) -> Result<BackendEvent> {
        self.error()
    }
    async fn create_calendar_item(
        &self,
        _: &str,
        _: &BackendCalendarMutation,
    ) -> Result<BackendEvent> {
        self.error()
    }
    async fn update_calendar_item(
        &self,
        _: &BackendEvent,
        _: &BackendCalendarMutation,
    ) -> Result<BackendEvent> {
        self.error()
    }
    async fn delete_calendar_item(&self, _: &BackendEvent) -> Result<()> {
        self.error()
    }
    async fn respond_calendar_item(
        &self,
        _: &BackendEvent,
        _: eas_mail_protocol::MeetingResponseChoice,
    ) -> Result<Option<String>> {
        self.error()
    }
    async fn respond_meeting_request(
        &self,
        _: &MailSource,
        _: eas_mail_protocol::MeetingResponseChoice,
    ) -> Result<Option<String>> {
        self.error()
    }
    async fn send_calendar_message(&self, _: &str, _: Vec<u8>) -> Result<()> {
        self.error()
    }
    async fn check_mail_property_ready(&self, _: &MailSource) -> Result<()> {
        self.error()
    }
    async fn mark_read(&self, _: &MailSource, _: bool) -> Result<()> {
        self.error()
    }
    async fn move_mail(&self, _: &MailSource, _: &str) -> Result<MailSource> {
        self.error()
    }
    async fn set_mail_flag(&self, _: &MailSource, _: u8) -> Result<()> {
        self.error()
    }
    async fn set_mail_categories(&self, _: &MailSource, _: &[String]) -> Result<()> {
        self.error()
    }
    async fn send(&self, _: &str, _: &OutgoingMail) -> Result<()> {
        self.error()
    }
    async fn reply(&self, _: &str, _: &MailSource, _: &OutgoingMail) -> Result<()> {
        self.error()
    }
    async fn forward(&self, _: &str, _: &MailSource, _: &OutgoingMail) -> Result<()> {
        self.error()
    }
}

#[cfg(test)]
mod tests;
