use std::sync::Arc;

use super::Runtime;
use super::calendar_series::edit::{self, EditInput, EditPlan, ItemAction};
use super::calendar_write_result::{self, STEP_NEW_SERIES};
use super::calendar_write_support::{required_notification, step_client_id};
use super::write_preview::{self, PreparedWrite};
use crate::backend::{AccountBackend, BackendEvent};
use crate::model::CalendarOperationResult;
use crate::{Result, Warning};

impl Runtime {
    pub(super) async fn calendar_edit(
        &self,
        input: EditInput,
        expected: Option<&str>,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        if let Some(record) = self.replay_write(input.name(), input.key(), &input)? {
            return Ok((calendar_write_result::existing(record), Vec::new()));
        }
        let reference = self.references.event(input.reference())?;
        let backend = self.require_write(&reference.account_id)?;
        let _guard = self.write_locks.acquire(&reference.account_id).await?;
        if let Some(record) = self.replay_write(input.name(), input.key(), &input)? {
            return Ok((calendar_write_result::existing(record), Vec::new()));
        }
        let mut source = self.account_result(
            &reference.account_id,
            backend.resolve_calendar_source(&reference).await,
        )?;
        source.occurrence_start = reference.occurrence_start;
        let plan = edit::plan(&input, &source, self.clock.now(), &backend.account().email)?;
        self.require_calendar_capabilities(&backend, plan.meeting).await?;
        write_preview::verify(&plan.preview, expected)?;
        self.execute_calendar_edit(&input, source, plan, backend).await
    }

    pub(super) async fn prepare_calendar_edit(
        &self,
        input: EditInput,
    ) -> Result<PreparedWrite<CalendarOperationResult>> {
        if let Some(record) = self.replay_write(input.name(), input.key(), &input)? {
            return Ok(PreparedWrite::Replay(calendar_write_result::existing(record)));
        }
        let reference = self.references.event(input.reference())?;
        let backend = self.require_write(&reference.account_id)?;
        let mut source = self.account_result(
            &reference.account_id,
            backend.resolve_calendar_source(&reference).await,
        )?;
        source.occurrence_start = reference.occurrence_start;
        let plan = edit::plan(&input, &source, self.clock.now(), &backend.account().email)?;
        self.require_calendar_capabilities(&backend, plan.meeting).await?;
        Ok(PreparedWrite::Ready(plan.preview))
    }

    async fn execute_calendar_edit(
        &self,
        input: &EditInput,
        source: BackendEvent,
        plan: EditPlan,
        backend: Arc<dyn AccountBackend>,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        // Render every message before beginning the operation; no validation failure may follow a mutation.
        let notices = plan
            .notices
            .iter()
            .map(|notice| {
                let mime = required_notification(
                    &backend.account().email,
                    &notice.event,
                    &notice.recipients,
                    notice.method,
                    input.comment(),
                )?;
                let client_id =
                    step_client_id(input.key(), &format!("notification-{}", notice.bit))?;
                Ok((notice.bit, client_id, mime))
            })
            .collect::<Result<Vec<_>>>()?;
        let new_client_id = step_client_id(input.key(), "new-series-item")?;
        let begin = self.begin_write(&source.account_id, input.name(), input.key(), input)?;
        if !begin.inserted {
            return Ok((calendar_write_result::existing(begin.record), Vec::new()));
        }
        let mut completed = 0;
        let mut event_ref = None;
        for step in plan.steps {
            let result = match step.action {
                ItemAction::Create(mut event) => {
                    event.mutation.target_collection.clone_from(&source.collection_id);
                    backend.create_calendar_item(&new_client_id, &event.mutation).await.map(Some)
                }
                ItemAction::Update(event) => {
                    backend.update_calendar_item(&source, &event.mutation).await.map(Some)
                }
                ItemAction::Delete => backend.delete_calendar_item(&source).await.map(|()| None),
            };
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    return self.calendar_failure(&begin.record, completed, error, event_ref);
                }
            };
            completed |= step.bit;
            self.journal.checkpoint(&begin.record.operation_id, completed)?;
            if step.bit == STEP_NEW_SERIES || completed & STEP_NEW_SERIES == 0 {
                event_ref = result
                    .map(|mut event| {
                        event.occurrence_start = plan.occurrence_start;
                        self.references.insert_event(event)
                    })
                    .transpose()?;
            }
        }
        for (bit, client_id, mime) in notices {
            if let Err(error) = backend.send_calendar_message(&client_id, mime).await {
                return self.calendar_failure(&begin.record, completed, error, event_ref);
            }
            completed |= bit;
            self.journal.checkpoint(&begin.record.operation_id, completed)?;
        }
        self.calendar_success(&begin.record, completed, event_ref)
    }
}
