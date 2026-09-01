use chrono::{DateTime, Utc};
use eas_mail_protocol::{Command, EasError, RecipientAvailability};

use super::super::{BackendCalendarSearch, BackendEvent};
use super::session::{EasMailbox, SessionState};
use crate::{AppError, ErrorCode, Result};

impl EasMailbox {
    pub(super) async fn directory_search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<eas_mail_protocol::protocol::DirectoryPage> {
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        let mut result = self.client.search_people(state.policy_key, query, limit).await;
        if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            result = self.client.search_people(state.policy_key, query, limit).await;
        }
        result.map_err(self.scoped_error())
    }

    pub(super) async fn availability(
        &self,
        participants: &[String],
        starts_at: DateTime<Utc>,
        ends_at: DateTime<Utc>,
    ) -> Result<Vec<RecipientAvailability>> {
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        self.require_calendar_availability(&state)?;
        let mut result =
            self.client.availability(state.policy_key, participants, starts_at, ends_at).await;
        if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            result =
                self.client.availability(state.policy_key, participants, starts_at, ends_at).await;
        }
        result.map_err(self.scoped_error())
    }

    pub(super) async fn search_events(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<BackendCalendarSearch> {
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        let mut result = self.client.search_calendar(state.policy_key, query, 0, limit).await;
        if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            result = self.client.search_calendar(state.policy_key, query, 0, limit).await;
        }
        let page = result.map_err(self.scoped_error())?;
        let events = page
            .items
            .into_iter()
            .map(|event| BackendEvent {
                occurrence_start: None,
                account_id: self.account.account_id.clone(),
                long_id: event.long_id,
                collection_id: event.collection_id,
                server_id: event.server_id,
                fields: event.fields,
            })
            .collect();
        Ok(BackendCalendarSearch { events, total: page.total })
    }

    pub(super) async fn fetch_event(
        &self,
        source: &BackendEvent,
        body_limit: usize,
    ) -> Result<BackendEvent> {
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        let body_limit = body_limit.min(policy(&state)?.body_limit);
        let long_id = (!source.long_id.is_empty()).then_some(source.long_id.as_str());
        let mut result = self
            .client
            .fetch_calendar_source(
                state.policy_key,
                long_id,
                source.collection_id.as_deref(),
                source.server_id.as_deref(),
                body_limit,
            )
            .await;
        if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            let body_limit = body_limit.min(policy(&state)?.body_limit);
            result = self
                .client
                .fetch_calendar_source(
                    state.policy_key,
                    long_id,
                    source.collection_id.as_deref(),
                    source.server_id.as_deref(),
                    body_limit,
                )
                .await;
        }
        let result = result.map_err(self.scoped_error())?;
        Ok(BackendEvent {
            occurrence_start: source.occurrence_start,
            account_id: self.account.account_id.clone(),
            long_id: source.long_id.clone(),
            collection_id: result.collection_id.or_else(|| source.collection_id.clone()),
            server_id: result.server_id.or_else(|| source.server_id.clone()),
            fields: result.fields,
        })
    }

    fn require_calendar_availability(&self, state: &SessionState) -> Result<()> {
        if state
            .capabilities
            .as_ref()
            .is_some_and(|value| value.supports(Command::ResolveRecipients))
        {
            return Ok(());
        }
        Err(AppError::new(
            ErrorCode::FeatureUnavailable,
            "Exchange does not advertise ResolveRecipients availability",
        )
        .account(&self.account.account_id)
        .remediation("Ask the Exchange administrator whether free/busy lookup is enabled"))
    }
}

fn policy(state: &SessionState) -> Result<&eas_mail_protocol::protocol::PolicyDecision> {
    state.policy.as_ref().ok_or_else(|| {
        AppError::new(ErrorCode::ProtocolError, "process-local Exchange state is inconsistent")
    })
}
