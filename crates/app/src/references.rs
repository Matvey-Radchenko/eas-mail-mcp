use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};

mod object;

pub(crate) use object::{AttachmentReference, MeetingReference};

use crate::backend::{BackendEvent, BackendMail};
use crate::model::{MailPage, MailSearchCoverage, MailSummary, Warning};
use crate::{AppError, ErrorCode, Result};

const LIFETIME_MINUTES: i64 = 15;
const PRUNE_INTERVAL_MINUTES: i64 = 1;
const MAX_SNAPSHOTS: usize = 32;

/// Time boundary used by runtime expiry logic and deterministic harnesses.
pub trait Clock: Send + Sync {
    /// Returns the current UTC time.
    fn now(&self) -> DateTime<Utc>;
}

/// Production UTC clock.
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Opaque identifier boundary used by runtime references and harnesses.
pub trait IdGenerator: Send + Sync {
    /// Returns an unpredictable or deterministic identifier.
    fn next(&self) -> String;
}

/// Production UUID generator.
#[derive(Debug, Default)]
pub struct RandomIds;

impl IdGenerator for RandomIds {
    fn next(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

#[derive(Clone)]
enum Snapshot {
    Mail { items: Arc<Vec<MailSummary>>, coverage: Vec<MailSearchCoverage>, warnings: Vec<Warning> },
}

#[derive(Clone)]
struct Cursor {
    snapshot: Snapshot,
    offset: usize,
}

struct Timed<T> {
    expires_at: DateTime<Utc>,
    value: T,
}

#[derive(Default)]
struct State {
    cursors: BTreeMap<String, Timed<Cursor>>,
    cursor_order: VecDeque<String>,
    last_pruned_at: Option<DateTime<Utc>>,
}

pub(super) struct References {
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGenerator>,
    state: Mutex<State>,
}

impl References {
    pub(super) fn new(clock: Arc<dyn Clock>, ids: Arc<dyn IdGenerator>) -> Self {
        Self { clock, ids, state: Mutex::new(State::default()) }
    }

    pub(super) fn insert_mail(&self, value: BackendMail) -> Result<String> {
        object::encode_mail(value)
    }

    pub(super) fn mail(&self, id: &str) -> Result<BackendMail> {
        object::decode_mail(id)
    }

    pub(super) fn insert_event(&self, value: BackendEvent) -> Result<String> {
        object::encode_event(value)
    }

    pub(super) fn event(&self, id: &str) -> Result<BackendEvent> {
        object::decode_event(id)
    }

    pub(super) fn meeting(&self, id: &str) -> Result<MeetingReference> {
        object::decode_meeting(id)
    }

    pub(super) fn insert_attachment(&self, value: AttachmentReference) -> Result<String> {
        object::encode_attachment(value)
    }

    pub(super) fn attachment(&self, id: &str) -> Result<AttachmentReference> {
        object::decode_attachment(id)
    }

    pub(super) fn next_token(&self, prefix: &str) -> String {
        format!("{prefix}_{}", self.ids.next())
    }

    pub(super) fn purge_account(&self, _account_id: &str) -> Result<()> {
        let mut state = self.lock()?;
        state.cursors.clear();
        state.cursor_order.clear();
        Ok(())
    }

    pub(super) fn first_mail_page(
        &self,
        items: Vec<MailSummary>,
        limit: usize,
    ) -> Result<(Vec<MailSummary>, Option<String>)> {
        self.first_page(
            Snapshot::Mail { items: Arc::new(items), coverage: Vec::new(), warnings: Vec::new() },
            limit,
        )
        .and_then(mail_page)
    }

    #[cfg(test)]
    pub(super) fn next_mail_page(
        &self,
        cursor: &str,
        limit: usize,
    ) -> Result<(Vec<MailSummary>, Option<String>)> {
        self.next_page(cursor, limit).and_then(mail_page)
    }

    pub(super) fn first_search_page(
        &self,
        items: Vec<MailSummary>,
        coverage: Vec<MailSearchCoverage>,
        warnings: Vec<Warning>,
        limit: usize,
    ) -> Result<(MailPage, Vec<Warning>)> {
        self.first_page(Snapshot::Mail { items: Arc::new(items), coverage, warnings }, limit)
            .and_then(search_page)
    }

    pub(super) fn next_search_page(
        &self,
        cursor: &str,
        limit: usize,
    ) -> Result<(MailPage, Vec<Warning>)> {
        self.next_page(cursor, limit).and_then(search_page)
    }

    fn first_page(&self, snapshot: Snapshot, limit: usize) -> Result<Page> {
        let length = snapshot_len(&snapshot);
        let end = length.min(limit);
        let next = (end < length).then(|| Cursor { snapshot: snapshot.clone(), offset: end });
        let next_cursor = next.map(|cursor| self.store_cursor(cursor)).transpose()?;
        Ok(Page { snapshot, start: 0, end, next_cursor })
    }

    fn next_page(&self, cursor_id: &str, limit: usize) -> Result<Page> {
        let now = self.clock.now();
        let mut state = self.lock()?;
        prune(&mut state, now);
        let cursor =
            state.cursors.remove(cursor_id).map(|entry| entry.value).ok_or_else(expired)?;
        state.cursor_order.retain(|value| value != cursor_id);
        drop(state);
        let length = snapshot_len(&cursor.snapshot);
        let end = cursor.offset.saturating_add(limit).min(length);
        let next =
            (end < length).then(|| Cursor { snapshot: cursor.snapshot.clone(), offset: end });
        let next_cursor = next.map(|value| self.store_cursor(value)).transpose()?;
        Ok(Page { snapshot: cursor.snapshot, start: cursor.offset, end, next_cursor })
    }

    fn store_cursor(&self, cursor: Cursor) -> Result<String> {
        let id = format!("cursor_{}", self.ids.next());
        let now = self.clock.now();
        let expires_at = expires_at(now);
        let mut state = self.lock()?;
        maybe_prune(&mut state, now);
        while state.cursor_order.len() >= MAX_SNAPSHOTS {
            if let Some(oldest) = state.cursor_order.pop_front() {
                state.cursors.remove(&oldest);
            }
        }
        state.cursor_order.push_back(id.clone());
        state.cursors.insert(id.clone(), Timed { expires_at, value: cursor });
        Ok(id)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>> {
        self.state.lock().map_err(|_| {
            AppError::new(ErrorCode::StorageError, "process-local reference store is unavailable")
        })
    }
}

struct Page {
    snapshot: Snapshot,
    start: usize,
    end: usize,
    next_cursor: Option<String>,
}

fn mail_page(page: Page) -> Result<(Vec<MailSummary>, Option<String>)> {
    match page.snapshot {
        Snapshot::Mail { items, .. } => {
            Ok((slice(&items, page.start, page.end)?, page.next_cursor))
        }
    }
}

fn search_page(page: Page) -> Result<(MailPage, Vec<Warning>)> {
    match page.snapshot {
        Snapshot::Mail { items, coverage, warnings } => {
            let results_truncated = coverage
                .iter()
                .any(|value| !value.candidates_complete || value.metadata_unknown != 0);
            Ok((
                MailPage {
                    items: slice(&items, page.start, page.end)?,
                    next_cursor: page.next_cursor,
                    results_truncated,
                    coverage,
                },
                warnings,
            ))
        }
    }
}

fn slice<T: Clone>(items: &[T], start: usize, end: usize) -> Result<Vec<T>> {
    items.get(start..end).map(<[T]>::to_vec).ok_or_else(expired)
}

fn snapshot_len(snapshot: &Snapshot) -> usize {
    match snapshot {
        Snapshot::Mail { items, .. } => items.len(),
    }
}

fn prune(state: &mut State, now: DateTime<Utc>) {
    state.cursors.retain(|_, value| value.expires_at > now);
    state.cursor_order.retain(|id| state.cursors.contains_key(id));
    state.last_pruned_at = Some(now);
}

fn maybe_prune(state: &mut State, now: DateTime<Utc>) {
    let interval = Duration::minutes(PRUNE_INTERVAL_MINUTES).num_seconds().unsigned_abs();
    let due = state.last_pruned_at.is_none_or(|last| {
        now.signed_duration_since(last).num_seconds().unsigned_abs() >= interval
    });
    if due {
        prune(state, now);
    }
}

fn expires_at(now: DateTime<Utc>) -> DateTime<Utc> {
    now + Duration::minutes(LIFETIME_MINUTES)
}

fn expired() -> AppError {
    AppError::new(
        ErrorCode::ReferenceExpired,
        "the process-local pagination cursor expired or was already consumed; run the list or search tool again",
    )
}

#[cfg(test)]
mod tests;
