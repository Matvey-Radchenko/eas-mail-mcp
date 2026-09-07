use super::fixtures::{event, personal_event, received_event, recurring_event};
use super::{FakeBackend, failure, oof};
use eas_mail_mcp::backend::{BackendAccount, BackendCapabilities, BackendEvent, OutgoingMail};
use eas_mail_mcp::{ErrorCode, Result};
use eas_mail_protocol::ProfileKey;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

impl FakeBackend {
    /// Creates a successful backend with write tools enabled.
    #[must_use]
    pub fn new(account_id: &str) -> Self {
        let calendar_items = [
            event(account_id),
            personal_event(account_id),
            received_event(account_id),
            recurring_event(account_id),
        ]
        .into_iter()
        .filter_map(|value| value.server_id.clone().map(|key| (key, value)))
        .collect();
        Self {
            account: BackendAccount {
                account_id: account_id.into(),
                profile: ProfileKey::default(),
                email: format!("{account_id}@example.invalid"),
                email_domains: vec!["example.invalid".into()],
                enabled: true,
                write_enabled: true,
            },
            failure: Mutex::new(None),
            operation_failure: Mutex::new(None),
            mail_count: 1,
            mail_items: Mutex::new(BTreeMap::new()),
            removed_mail: Mutex::new(std::collections::BTreeSet::new()),
            include_series: false,
            operations: Mutex::new(Vec::new()),
            calendar_items: Mutex::new(calendar_items),
            calendar_messages: Mutex::new(Vec::new()),
            outgoing_messages: Mutex::new(Vec::new()),
            calendar_responses: Mutex::new(Vec::new()),
            source_resolutions: AtomicUsize::new(0),
            created_events: AtomicUsize::new(0),
            capabilities: BackendCapabilities {
                calendar_availability: true,
                mail_writes: true,
                personal_calendar_writes: true,
                meeting_lifecycle: true,
                auto_reply: true,
                mail_move: true,
                mail_properties: true,
            },
            delay: Duration::ZERO,
            oof: Mutex::new(oof::OofFixture::default()),
        }
    }

    /// Creates a backend that returns a retryable network error.
    #[must_use]
    pub fn failing(account_id: &str) -> Self {
        Self { failure: Mutex::new(Some(ErrorCode::NetworkUnreachable)), ..Self::new(account_id) }
    }

    /// Configures the number of deterministic messages returned by list and search.
    #[must_use]
    pub const fn with_mail_count(mut self, count: usize) -> Self {
        self.mail_count = count;
        self
    }

    /// Enables or disables account-level write tools.
    #[must_use]
    pub const fn with_writes_enabled(mut self, enabled: bool) -> Self {
        self.account.write_enabled = enabled;
        self
    }

    /// Replaces safe account identity metadata for account-selection tests.
    #[must_use]
    pub fn with_identity(mut self, email: &str, domains: &[&str]) -> Self {
        self.account.email = email.into();
        self.account.email_domains = domains.iter().map(|value| (*value).into()).collect();
        self
    }

    /// Adds deterministic latency to each asynchronous backend operation.
    #[must_use]
    pub const fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Replaces Calendar write capability flags for preflight tests.
    #[must_use]
    pub const fn with_calendar_capabilities(mut self, personal: bool, meeting: bool) -> Self {
        self.capabilities.personal_calendar_writes = personal;
        self.capabilities.meeting_lifecycle = meeting;
        self
    }

    /// Selects a deterministic account failure or restores normal operation.
    pub fn set_failure(&self, value: Option<ErrorCode>) -> Result<()> {
        *self.failure.lock().map_err(|_| failure(ErrorCode::StorageError))? = value;
        Ok(())
    }

    /// Fails one named operation until the failure is explicitly cleared.
    pub fn set_operation_failure(&self, name: Option<&str>, code: ErrorCode) -> Result<()> {
        *self.operation_failure.lock().map_err(|_| failure(ErrorCode::StorageError))? =
            name.map(|value| (value.to_owned(), 0, code));
        Ok(())
    }

    /// Returns mutation names recorded by the fake backend.
    pub fn operations(&self) -> Result<Vec<String>> {
        self.operations
            .lock()
            .map(|values| values.clone())
            .map_err(|_| failure(ErrorCode::StorageError))
    }

    /// Returns exact outgoing messages received by the compose boundary.
    pub fn outgoing_messages(&self) -> Result<Vec<OutgoingMail>> {
        self.outgoing_messages
            .lock()
            .map(|values| values.clone())
            .map_err(|_| failure(ErrorCode::StorageError))
    }

    /// Returns how many mutable-source resolutions were attempted.
    #[must_use]
    pub fn source_resolutions(&self) -> usize {
        self.source_resolutions.load(Ordering::Relaxed)
    }

    pub(super) async fn check(&self) -> Result<()> {
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        self.failure
            .lock()
            .map_err(|_| failure(ErrorCode::StorageError))?
            .map_or(Ok(()), |code| Err(failure(code)))
    }

    pub(super) async fn check_operation(&self, name: &str) -> Result<()> {
        self.check().await?;
        let mut scripted =
            self.operation_failure.lock().map_err(|_| failure(ErrorCode::StorageError))?;
        if let Some((expected, remaining, code)) = scripted.as_mut()
            && expected == name
        {
            if *remaining == 0 {
                return Err(failure(*code));
            }
            *remaining -= 1;
        }
        Ok(())
    }

    pub(super) fn record(&self, value: &str) -> Result<()> {
        self.operations.lock().map_err(|_| failure(ErrorCode::StorageError))?.push(value.into());
        Ok(())
    }

    pub(super) fn calendar_item(&self, source: &BackendEvent) -> Result<BackendEvent> {
        let key = source
            .server_id
            .as_deref()
            .filter(|value| !value.is_empty())
            .or_else(|| (!source.long_id.is_empty()).then_some(source.long_id.as_str()))
            .ok_or_else(|| failure(ErrorCode::NotFound))?;
        self.calendar_items
            .lock()
            .map_err(|_| failure(ErrorCode::StorageError))?
            .get(key)
            .cloned()
            .ok_or_else(|| failure(ErrorCode::NotFound))
    }

    pub(super) fn store_calendar_item(&self, value: BackendEvent) -> Result<()> {
        let key = value.server_id.clone().ok_or_else(|| failure(ErrorCode::ProtocolError))?;
        self.calendar_items
            .lock()
            .map_err(|_| failure(ErrorCode::StorageError))?
            .insert(key, value);
        Ok(())
    }

    pub(super) fn remove_calendar_item(&self, source: &BackendEvent) -> Result<()> {
        let key = source
            .server_id
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| failure(ErrorCode::NotFound))?;
        if !key.starts_with("event-created") {
            return Ok(());
        }
        self.calendar_items
            .lock()
            .map_err(|_| failure(ErrorCode::StorageError))?
            .remove(key)
            .map(|_| ())
            .ok_or_else(|| failure(ErrorCode::NotFound))
    }
}
