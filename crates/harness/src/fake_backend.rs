use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use eas_mail_mcp::backend::{
    AccountBackend, BackendAccount, BackendEvent, BackendMail, BackendSync, MailSource,
    OutgoingMail,
};
use eas_mail_mcp::{AppError, ErrorCode, Result};
use eas_mail_protocol::{
    Attachment, CalendarFields, CollectionKind, Folder, MailFields, Patch, ProfileKey,
};

/// Deterministic high-level backend used by MCP black-box tests.
#[derive(Debug)]
pub struct FakeBackend {
    account: BackendAccount,
    failure: Mutex<Option<ErrorCode>>,
    mail_count: usize,
    operations: Mutex<Vec<String>>,
    delay: Duration,
}

impl FakeBackend {
    /// Creates a successful backend with write tools enabled.
    #[must_use]
    pub fn new(account_id: &str) -> Self {
        Self {
            account: BackendAccount {
                account_id: account_id.into(),
                profile: ProfileKey::default(),
                email: format!("{account_id}@example.invalid"),
                enabled: true,
                write_enabled: true,
            },
            failure: Mutex::new(None),
            mail_count: 1,
            operations: Mutex::new(Vec::new()),
            delay: Duration::ZERO,
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

    /// Adds deterministic latency to each asynchronous backend operation.
    #[must_use]
    pub const fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Selects a deterministic account failure or restores normal operation.
    pub fn set_failure(&self, value: Option<ErrorCode>) -> Result<()> {
        *self.failure.lock().map_err(|_| failure(ErrorCode::StorageError))? = value;
        Ok(())
    }

    /// Returns mutation names recorded by the fake backend.
    pub fn operations(&self) -> Result<Vec<String>> {
        self.operations
            .lock()
            .map(|values| values.clone())
            .map_err(|_| failure(ErrorCode::StorageError))
    }

    async fn check(&self) -> Result<()> {
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        self.failure
            .lock()
            .map_err(|_| failure(ErrorCode::StorageError))?
            .map_or(Ok(()), |code| Err(failure(code)))
    }

    fn record(&self, value: &str) -> Result<()> {
        self.operations.lock().map_err(|_| failure(ErrorCode::StorageError))?.push(value.into());
        Ok(())
    }
}

#[async_trait]
impl AccountBackend for FakeBackend {
    fn account(&self) -> BackendAccount {
        self.account.clone()
    }

    async fn folders(&self) -> Result<Vec<Folder>> {
        self.check().await?;
        Ok(folders())
    }

    async fn sync(&self, mail: bool, calendar: bool) -> Result<BackendSync> {
        self.check().await?;
        Ok(BackendSync {
            collections: usize::from(mail) + usize::from(calendar),
            changes: usize::from(mail) + usize::from(calendar),
        })
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

    async fn search_mail(&self, _: &str, _: usize) -> Result<Vec<BackendMail>> {
        self.check().await?;
        Ok((0..self.mail_count)
            .map(|index| {
                mail(&self.account.account_id, MailSource::LongId(format!("long-message-{index}")))
            })
            .collect())
    }

    async fn fetch_mail(&self, source: &MailSource, _: usize) -> Result<BackendMail> {
        self.check().await?;
        Ok(mail(&self.account.account_id, source.clone()))
    }

    async fn fetch_attachment(&self, _: &str) -> Result<Vec<u8>> {
        self.check().await?;
        Ok(b"attachment payload".to_vec())
    }

    async fn list_calendar(&self, folder_ids: Option<&[String]>) -> Result<Vec<BackendEvent>> {
        self.check().await?;
        Ok(if folder_ids.is_none_or(|ids| ids.iter().any(|id| id == "calendar")) {
            vec![event(&self.account.account_id)]
        } else {
            Vec::new()
        })
    }

    async fn mark_read(&self, _: &MailSource, _: bool) -> Result<()> {
        self.check().await?;
        self.record("mail_mark_read")
    }

    async fn send(&self, _: &str, _: &OutgoingMail) -> Result<()> {
        self.check().await?;
        self.record("mail_send")
    }

    async fn reply(&self, _: &str, _: &MailSource, _: &OutgoingMail) -> Result<()> {
        self.check().await?;
        self.record("mail_reply")
    }

    async fn forward(&self, _: &str, _: &MailSource, _: &OutgoingMail) -> Result<()> {
        self.check().await?;
        self.record("mail_forward")
    }
}

fn folders() -> Vec<Folder> {
    vec![
        Folder {
            server_id: "inbox".into(),
            parent_id: "0".into(),
            display_name: "Inbox".into(),
            folder_type: 2,
            kind: Some(CollectionKind::Mail),
        },
        Folder {
            server_id: "calendar".into(),
            parent_id: "0".into(),
            display_name: "Calendar".into(),
            folder_type: 8,
            kind: Some(CollectionKind::Calendar),
        },
    ]
}

fn mail(account_id: &str, source: MailSource) -> BackendMail {
    BackendMail {
        account_id: account_id.into(),
        folder_id: match &source {
            MailSource::Item { folder_id, .. } => folder_id.clone(),
            MailSource::LongId(_) => String::new(),
        },
        source,
        fields: MailFields {
            subject: Patch::Value("Quarterly update".into()),
            sender: Patch::Value("Sender <sender@example.invalid>".into()),
            recipients: Patch::Value(format!("{account_id}@example.invalid")),
            cc: Patch::Value(String::new()),
            received_at: Patch::Value(chrono::DateTime::from_timestamp(1_700_000_000, 0)),
            body: Patch::Value("<p>Safe <strong>plain</strong> body</p>".into()),
            body_truncated: Patch::Value(false),
            is_read: Patch::Value(false),
            importance: Patch::Value(1),
            attachments: Patch::Value(vec![Attachment {
                display_name: "report.txt".into(),
                file_reference: "attachment-1".into(),
                size: 18,
                content_type: "text/plain".into(),
                is_inline: false,
                content_id: String::new(),
            }]),
        },
    }
}

fn event(account_id: &str) -> BackendEvent {
    BackendEvent {
        account_id: account_id.into(),
        folder_id: "calendar".into(),
        server_id: "event-1".into(),
        fields: CalendarFields {
            subject: Patch::Value("Planning".into()),
            body: Patch::Value("<p>Agenda</p>".into()),
            starts_at: Patch::Value(chrono::DateTime::from_timestamp(1_700_010_000, 0)),
            ends_at: Patch::Value(chrono::DateTime::from_timestamp(1_700_013_600, 0)),
            all_day: Patch::Value(false),
            location: Patch::Value("Room 1".into()),
            organizer: Patch::Value("owner@example.invalid".into()),
            attendees: Patch::Value(vec!["guest@example.invalid".into()]),
            reminder_minutes: Patch::Value(15),
            recurrence: Patch::Value(BTreeMap::new()),
            exceptions: Patch::Value(Vec::new()),
            meeting_status: Patch::Value(1),
        },
    }
}

fn failure(code: ErrorCode) -> AppError {
    let error = AppError::new(code, "scripted backend is unavailable");
    if code == ErrorCode::NetworkUnreachable { error.retryable() } else { error }
}
