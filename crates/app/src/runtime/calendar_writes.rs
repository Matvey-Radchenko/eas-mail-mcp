use std::sync::Arc;

use super::Runtime;
use super::calendar_mime::CalendarMessageMethod;
use super::calendar_prepare;
use super::calendar_write_preview::event_preview;
use super::calendar_write_result::{self, STEP_ITEM, STEP_NOTIFY_CURRENT};
use super::calendar_write_support::{notification, operation_uid, step_client_id};
use super::write_preview;
use crate::backend::AccountBackend;
use crate::model::{
    ApiResponse, CalendarCancelInput, CalendarCreateInput, CalendarDeleteInput,
    CalendarOperationResult, CalendarOperationState, CalendarRespondInput, CalendarUpdateInput,
};
use crate::{AppError, ErrorCode, JournalRecord, OperationStatus, Result, Warning};

impl Runtime {
    /// Creates one personal event or organizer meeting.
    pub async fn calendar_create(
        &self,
        input: CalendarCreateInput,
    ) -> ApiResponse<CalendarOperationResult> {
        Self::response(self.calendar_create_result(input, None).await)
    }

    /// Applies a patch to one personal event or organizer meeting.
    pub async fn calendar_update(
        &self,
        input: CalendarUpdateInput,
    ) -> ApiResponse<CalendarOperationResult> {
        Self::response(self.calendar_update_result(input, None).await)
    }

    /// Deletes one personal event.
    pub async fn calendar_delete(
        &self,
        input: CalendarDeleteInput,
    ) -> ApiResponse<CalendarOperationResult> {
        Self::response(self.calendar_delete_result(input, None).await)
    }

    /// Cancels one organizer meeting and notifies attendees.
    pub async fn calendar_cancel(
        &self,
        input: CalendarCancelInput,
    ) -> ApiResponse<CalendarOperationResult> {
        Self::response(self.calendar_cancel_result(input, None).await)
    }

    /// Accepts, tentatively accepts, or declines one received meeting.
    pub async fn calendar_respond(
        &self,
        input: CalendarRespondInput,
    ) -> ApiResponse<CalendarOperationResult> {
        Self::response(self.calendar_respond_result(input, None).await)
    }

    pub(crate) async fn commit_cli_calendar_create(
        &self,
        input: CalendarCreateInput,
        expected: &str,
    ) -> ApiResponse<CalendarOperationResult> {
        Self::response(self.calendar_create_result(input, Some(expected)).await)
    }

    pub(crate) async fn commit_cli_calendar_update(
        &self,
        input: CalendarUpdateInput,
        expected: &str,
    ) -> ApiResponse<CalendarOperationResult> {
        Self::response(self.calendar_update_result(input, Some(expected)).await)
    }

    pub(crate) async fn commit_cli_calendar_delete(
        &self,
        input: CalendarDeleteInput,
        expected: &str,
    ) -> ApiResponse<CalendarOperationResult> {
        Self::response(self.calendar_delete_result(input, Some(expected)).await)
    }

    pub(crate) async fn commit_cli_calendar_cancel(
        &self,
        input: CalendarCancelInput,
        expected: &str,
    ) -> ApiResponse<CalendarOperationResult> {
        Self::response(self.calendar_cancel_result(input, Some(expected)).await)
    }

    pub(crate) async fn commit_cli_calendar_respond(
        &self,
        input: CalendarRespondInput,
        expected: &str,
    ) -> ApiResponse<CalendarOperationResult> {
        Self::response(self.calendar_respond_result(input, Some(expected)).await)
    }

    async fn calendar_create_result(
        &self,
        input: CalendarCreateInput,
        expected: Option<&str>,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        if let Some(record) =
            self.replay_write("calendar_create", &input.idempotency_key, &input)?
        {
            return Ok((calendar_write_result::existing(record), Vec::new()));
        }
        let backend = self.require_write(&input.account_id)?;
        let account = backend.account();
        let uid = operation_uid(&input.idempotency_key)?;
        let prepared = calendar_prepare::create(&input, self.clock.now(), uid, &account.email)?;
        self.require_calendar_capabilities(
            &backend,
            !prepared.mutation.application.attendees.is_empty(),
        )
        .await?;
        let request_id = step_client_id(&input.idempotency_key, "request")?;
        let request_mime = notification(
            &account.email,
            &prepared,
            &prepared.mutation.application.attendees,
            CalendarMessageMethod::Request,
            "",
        )?;
        let _guard = self.write_locks.acquire(&input.account_id).await?;
        let backend = self.require_write(&input.account_id)?;
        write_preview::verify(
            &event_preview("calendar_create", &input.account_id, &prepared),
            expected,
        )?;
        let begin =
            self.begin_write(&input.account_id, "calendar_create", &input.idempotency_key, &input)?;
        if !begin.inserted {
            return Ok((calendar_write_result::existing(begin.record), Vec::new()));
        }
        let created =
            match backend.create_calendar_item(&begin.record.client_id, &prepared.mutation).await {
                Ok(value) => value,
                Err(error) => return self.calendar_failure(&begin.record, 0, error, None),
            };
        let mut steps = STEP_ITEM;
        self.checkpoint_mutation(&begin.record, steps)?;
        let event_ref = Self::journal_after_mutation(
            self.references.insert_event(created),
            &begin.record.account_id,
            &begin.record.operation_id,
        )?;
        if let Some(mime) = request_mime {
            if let Err(error) = backend.send_calendar_message(&request_id, mime).await {
                return self.calendar_failure(&begin.record, steps, error, Some(event_ref));
            }
            steps |= STEP_NOTIFY_CURRENT;
            self.checkpoint_mutation(&begin.record, steps)?;
        }
        self.calendar_success(&begin.record, steps, Some(event_ref))
    }

    async fn calendar_update_result(
        &self,
        input: CalendarUpdateInput,
        expected: Option<&str>,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        self.calendar_edit(
            super::calendar_series::edit::EditInput::Update(Box::new(input)),
            expected,
        )
        .await
    }

    async fn calendar_delete_result(
        &self,
        input: CalendarDeleteInput,
        expected: Option<&str>,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        self.calendar_edit(super::calendar_series::edit::EditInput::Delete(input), expected).await
    }

    async fn calendar_cancel_result(
        &self,
        input: CalendarCancelInput,
        expected: Option<&str>,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        self.calendar_edit(super::calendar_series::edit::EditInput::Cancel(input), expected).await
    }

    pub(super) fn calendar_success(
        &self,
        record: &JournalRecord,
        steps: u32,
        event_ref: Option<String>,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        Self::journal_after_mutation(
            self.journal.finish(&record.operation_id, OperationStatus::Succeeded, steps),
            &record.account_id,
            &record.operation_id,
        )?;
        Ok((
            calendar_write_result::result(
                &record.operation_id,
                CalendarOperationState::Succeeded,
                steps,
                "Exchange confirmed every Calendar operation step",
                event_ref,
            ),
            Vec::new(),
        ))
    }

    pub(super) fn calendar_failure(
        &self,
        record: &JournalRecord,
        steps: u32,
        error: AppError,
        event_ref: Option<String>,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        if error.envelope.code == ErrorCode::RemoteWipe {
            Self::journal_after_mutation(
                self.purge_account(&record.account_id),
                &record.account_id,
                &record.operation_id,
            )?;
            return Err(error.operation(&record.operation_id));
        }
        let (journal_status, result_status, message) = if error.envelope.code
            == ErrorCode::OutcomeUnknown
        {
            (
                OperationStatus::Unknown,
                CalendarOperationState::Unknown,
                "A Calendar operation step may have reached Exchange; do not retry with a new UUID",
            )
        } else if steps == 0 {
            (
                OperationStatus::Failed,
                CalendarOperationState::Failed,
                "Exchange safely rejected the Calendar operation",
            )
        } else {
            (
                OperationStatus::Partial,
                CalendarOperationState::Partial,
                "Some Calendar steps succeeded; do not retry with a new UUID",
            )
        };
        Self::journal_after_mutation(
            self.journal.finish(&record.operation_id, journal_status, steps),
            &record.account_id,
            &record.operation_id,
        )?;
        Ok((
            calendar_write_result::result(
                &record.operation_id,
                result_status,
                steps,
                message,
                event_ref,
            ),
            Vec::new(),
        ))
    }

    pub(super) async fn require_calendar_capabilities(
        &self,
        backend: &Arc<dyn AccountBackend>,
        meeting: bool,
    ) -> Result<()> {
        let account_id = backend.account().account_id;
        let capabilities = self.account_result(&account_id, backend.capabilities().await)?;
        let supported =
            capabilities.personal_calendar_writes && (!meeting || capabilities.meeting_lifecycle);
        if supported {
            Ok(())
        } else {
            let feature = if meeting { "meeting lifecycle" } else { "personal Calendar writes" };
            Err(AppError::new(
                ErrorCode::FeatureUnavailable,
                format!("Exchange does not advertise {feature}"),
            )
            .account(account_id))
        }
    }
}
