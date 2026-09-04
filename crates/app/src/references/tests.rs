use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Duration, Utc};
use eas_mail_protocol::MailFields;

use super::*;
use crate::backend::MailSource;

#[derive(Debug)]
struct ManualClock(Mutex<DateTime<Utc>>);

impl ManualClock {
    fn advance(&self, duration: Duration) -> Result<()> {
        let mut now = self.0.lock().map_err(|_| state_error())?;
        *now += duration;
        Ok(())
    }

    fn set(&self, value: DateTime<Utc>) -> Result<()> {
        *self.0.lock().map_err(|_| state_error())? = value;
        Ok(())
    }
}

impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        self.0.lock().map_or(DateTime::UNIX_EPOCH, |value| *value)
    }
}

#[derive(Debug, Default)]
struct SequenceIds(AtomicU64);

impl IdGenerator for SequenceIds {
    fn next(&self) -> String {
        self.0.fetch_add(1, Ordering::Relaxed).to_string()
    }
}

#[test]
fn object_references_survive_cursor_ttl() -> Result<()> {
    let clock = Arc::new(ManualClock(Mutex::new(DateTime::UNIX_EPOCH)));
    let references = References::new(clock.clone(), Arc::new(SequenceIds::default()));
    let mail_ref = references.insert_mail(mail("message-1"))?;
    let (_, cursor) = references.first_mail_page(summaries(), 1)?;

    clock.advance(Duration::minutes(LIFETIME_MINUTES + 1))?;

    assert_eq!(references.mail(&mail_ref)?.source, mail("message-1").source);
    let Err(error) = references.next_mail_page(&required(cursor)?, 1) else {
        return Err(AppError::new(ErrorCode::ProtocolError, "expired cursor was accepted"));
    };
    assert_eq!(error.envelope.code, ErrorCode::ReferenceExpired);
    Ok(())
}

#[test]
fn insertion_prunes_expired_cursors_and_keeps_current_snapshot() -> Result<()> {
    let clock = Arc::new(ManualClock(Mutex::new(DateTime::UNIX_EPOCH)));
    let references = References::new(clock.clone(), Arc::new(SequenceIds::default()));
    let (_, old_cursor) = references.first_mail_page(summaries(), 1)?;
    clock.advance(Duration::minutes(LIFETIME_MINUTES))?;
    let (_, current_cursor) = references.first_mail_page(summaries(), 1)?;

    let state = references.lock()?;
    assert_eq!(state.cursors.len(), 1);
    assert!(!state.cursors.contains_key(&required(old_cursor)?));
    assert!(state.cursors.contains_key(&required(current_cursor)?));
    Ok(())
}

#[test]
fn backward_clock_jump_restarts_the_cursor_prune_interval() -> Result<()> {
    let future = DateTime::UNIX_EPOCH + Duration::hours(1);
    let clock = Arc::new(ManualClock(Mutex::new(future)));
    let references = References::new(clock.clone(), Arc::new(SequenceIds::default()));
    let _ = references.first_mail_page(summaries(), 1)?;

    clock.set(DateTime::UNIX_EPOCH)?;
    let _ = references.first_mail_page(summaries(), 1)?;
    assert_eq!(references.lock()?.last_pruned_at, Some(DateTime::UNIX_EPOCH));
    Ok(())
}

fn summaries() -> Vec<MailSummary> {
    ["first", "second"]
        .into_iter()
        .map(|mail_ref| MailSummary {
            mail_ref: mail_ref.into(),
            account_id: "account".into(),
            folder_id: "inbox".into(),
            subject: String::new(),
            sender: String::new(),
            recipients: String::new(),
            received_at: None,
            preview: String::new(),
            is_read: false,
            has_attachments: false,
            flag: None,
            categories: None,
            calendar_message: None,
            can_respond: false,
            untrusted_external_content: true,
        })
        .collect()
}

fn required(value: Option<String>) -> Result<String> {
    value.ok_or_else(|| AppError::new(ErrorCode::ProtocolError, "test cursor is missing"))
}

fn mail(server_id: &str) -> BackendMail {
    BackendMail {
        account_id: "account".into(),
        folder_id: "inbox".into(),
        source: MailSource::Item { folder_id: "inbox".into(), server_id: server_id.into() },
        fields: MailFields::default(),
    }
}

fn state_error() -> AppError {
    AppError::new(ErrorCode::StorageError, "test clock is unavailable")
}
