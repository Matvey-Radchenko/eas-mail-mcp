use std::sync::Arc;
use std::time::Duration;

use chrono::Days;
use eas_mail_protocol::Patch;

use super::{Runtime, calendar_agenda, convert};
use crate::backend::{AccountBackend, BackendEvent};
use crate::calendar_cache::AccountCache;
use crate::model::{
    CalendarSearchInput, CalendarSyncCollection, CalendarSyncData, CalendarSyncInput,
    CalendarSyncItem,
};
use crate::{ApiResponse, AppError, ErrorCode, Result};

#[derive(Default)]
struct Statistics {
    pages: usize,
    changes: usize,
}

impl Runtime {
    /// Opts into persistent Calendar Sync. Omitted keys resume the saved baseline.
    pub async fn calendar_sync(&self, input: CalendarSyncInput) -> ApiResponse<CalendarSyncData> {
        Self::response(self.calendar_sync_result(input).await)
    }

    async fn calendar_sync_result(
        &self,
        input: CalendarSyncInput,
    ) -> Result<(CalendarSyncData, Vec<crate::Warning>)> {
        let plan = self.sync_window(&input)?;
        let max_pages = input.max_pages.unwrap_or(20);
        if !(1..=100).contains(&max_pages) || (input.full && input.sync_key.is_some()) {
            return Err(invalid("Use 1-100 pages; full and sync_key cannot be combined"));
        }
        let backend = self.backend(&input.account_id)?;
        let _guard = self.write_locks.acquire(&input.account_id).await?;
        let account = backend.account();
        let identity = format!("{}:{}", account.profile, account.email.to_lowercase());
        let mut cache = self.calendar_cache.load(&input.account_id, &identity)?;
        let result = backend.calendar_sync_sources().await;
        let sources = self.account_result(&input.account_id, result)?;
        let selected = sources
            .iter()
            .filter(|(id, _)| input.collection_id.as_ref().is_none_or(|value| value == id))
            .cloned()
            .collect::<Vec<_>>();
        if selected.is_empty() || (input.sync_key.is_some() && selected.len() != 1) {
            return Err(invalid("Select exactly one existing collection when supplying sync_key"));
        }
        // A successful hierarchy response, not a network failure, establishes removals.
        cache.collections.retain(|id, _| sources.iter().any(|(source, _)| source == id));
        let mut statistics = Statistics::default();
        for (id, filter) in &selected {
            let collection = cache.collections.entry(id.clone()).or_default();
            if let Some(key) = &input.sync_key
                && (key.len() > 4096 || key != collection.key() || collection.filter != *filter)
            {
                return Err(invalid("sync_key does not match the saved collection snapshot"));
            }
            if input.full
                || collection.filter != *filter
                || (collection.current.is_none() && collection.rebuilding.is_none())
            {
                collection.restart(*filter);
            }
            self.calendar_cache.save(&input.account_id, &cache)?;
            self.drain_calendar(Arc::clone(&backend), id, max_pages, &mut cache, &mut statistics)
                .await?;
        }
        let collections = selected
            .iter()
            .filter_map(|(id, _)| {
                cache.collections.get(id).map(|value| CalendarSyncCollection {
                    collection_id: id.clone(),
                    ready: value.current.is_some(),
                    more_available: value.more_available,
                    event_count: value.current.as_ref().map_or(0, |snapshot| snapshot.events.len()),
                    filter_type: value.filter,
                })
            })
            .collect::<Vec<_>>();
        let ready = collections.iter().all(|value| value.ready);
        let complete = ready && collections.iter().all(|value| !value.more_available);
        let items =
            self.project_calendar(&input.account_id, &account.email, &selected, &cache, &plan)?;
        Ok((
            CalendarSyncData {
                account_id: input.account_id,
                ready,
                complete,
                pages: statistics.pages,
                changes_applied: statistics.changes,
                items,
                collections,
            },
            Vec::new(),
        ))
    }

    async fn drain_calendar(
        &self,
        backend: Arc<dyn AccountBackend>,
        id: &str,
        max_pages: u16,
        cache: &mut AccountCache,
        statistics: &mut Statistics,
    ) -> Result<()> {
        let account_id = backend.account().account_id;
        let mut recovered = false;
        for _ in 0..max_pages {
            let mut candidate = cache
                .collections
                .get(id)
                .cloned()
                .ok_or_else(|| invalid("Calendar collection disappeared"))?;
            statistics.pages += 1;
            let result = tokio::time::timeout(
                Duration::from_secs(30),
                backend.calendar_sync_page(id, candidate.key()),
            )
            .await
            .map_err(|_| {
                AppError::new(
                    ErrorCode::ServiceUnavailable,
                    "Calendar Sync page timed out; saved state retained",
                )
                .retryable()
            })?;
            let result =
                self.account_result(&account_id, result).and_then(|page| candidate.apply(page));
            match result {
                Ok(count) => {
                    statistics.changes += count;
                }
                Err(error) if error.envelope.code == ErrorCode::SyncStale && !recovered => {
                    // Do not replace a valid old snapshot until the new baseline is drained.
                    candidate = cache
                        .collections
                        .get(id)
                        .cloned()
                        .ok_or_else(|| invalid("Calendar collection disappeared"))?;
                    candidate.restart(candidate.filter);
                    recovered = true;
                }
                Err(error) => return Err(error),
            }
            let more = candidate.more_available;
            cache.collections.insert(id.to_owned(), candidate);
            self.calendar_cache.save(&account_id, cache)?;
            if !more {
                return Ok(());
            }
        }
        Ok(())
    }

    fn sync_window(&self, input: &CalendarSyncInput) -> Result<calendar_agenda::AgendaPlan> {
        let zone = input
            .time_zone
            .as_deref()
            .unwrap_or("UTC")
            .parse::<chrono_tz::Tz>()
            .map_err(|_| invalid("time_zone must be an IANA timezone"))?;
        let today = self.clock.now().with_timezone(&zone).date_naive();
        let from = today.checked_sub_days(Days::new(90)).ok_or_else(|| invalid("Date overflow"))?;
        let to = today.checked_add_days(Days::new(90)).ok_or_else(|| invalid("Date overflow"))?;
        calendar_agenda::sync_plan(&CalendarSearchInput {
            account_ids: None,
            query: None,
            limit: None,
            date_from: Some(input.date_from.clone().unwrap_or_else(|| from.to_string())),
            date_to: Some(input.date_to.clone().unwrap_or_else(|| to.to_string())),
            time_zone: Some(zone.to_string()),
        })
    }

    fn project_calendar(
        &self,
        account_id: &str,
        email: &str,
        selected: &[(String, u8)],
        cache: &AccountCache,
        plan: &calendar_agenda::AgendaPlan,
    ) -> Result<Vec<CalendarSyncItem>> {
        let mut masters = Vec::new();
        for (id, _) in selected {
            if let Some(snapshot) =
                cache.collections.get(id).and_then(|value| value.current.as_ref())
            {
                for (server_id, fields) in &snapshot.events {
                    masters.push(BackendEvent {
                        occurrence_start: None,
                        account_id: account_id.into(),
                        long_id: String::new(),
                        collection_id: Some(id.clone()),
                        server_id: Some(server_id.clone()),
                        fields: fields.clone(),
                    });
                }
            }
        }
        let events = plan.apply(masters)?;
        if events.len() > 30_000 {
            return Err(AppError::new(
                ErrorCode::ResultTooLarge,
                "Calendar display window exceeds 30000 occurrences",
            ));
        }
        events
            .into_iter()
            .map(|event| {
                let reference = self.references.insert_event(event.clone())?;
                let summary = convert::calendar_event_summary(reference.clone(), &event);
                let reminder = match event.fields.reminder_minutes {
                    Patch::Value(value) => value,
                    Patch::Missing => None,
                };
                Ok(CalendarSyncItem {
                    body_available: matches!(event.fields.body, Patch::Value(_))
                        && !matches!(event.fields.body_truncated, Patch::Value(true)),
                    recurring: summary.recurring,
                    reminder,
                    event: convert::calendar_event(reference, &event, email, 500),
                })
            })
            .collect()
    }
}

fn invalid(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}
