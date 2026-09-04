use super::Runtime;
use super::calendar_mime::CalendarMessageMethod;
use super::calendar_prepare::{self, EventOwnership};
use super::calendar_response_prepare;
use super::calendar_write_preview::response_preview;
use super::calendar_write_result::{self, STEP_REPLY, STEP_RESPONSE};
use super::calendar_write_support::{
    organizer, required_notification, response_choice, response_reference, step_client_id,
};
use super::write_preview;
use crate::backend::{BackendEvent, BackendMail};
use crate::model::{CalendarOperationResult, CalendarRespondInput};
use crate::references::MeetingReference;
use crate::{Result, Warning};

impl Runtime {
    pub(super) async fn calendar_respond_result(
        &self,
        input: CalendarRespondInput,
        expected: Option<&str>,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        if let Some(record) =
            self.replay_write("calendar_respond", &input.idempotency_key, &input)?
        {
            return Ok((calendar_write_result::existing(record), Vec::new()));
        }
        calendar_prepare::validate_comment(&input.comment)?;
        match self.references.meeting(&input.event_ref)? {
            MeetingReference::Event(reference) => {
                self.respond_to_event(&input, reference, expected).await
            }
            MeetingReference::Mail(reference) => {
                self.respond_to_mail(&input, reference, expected).await
            }
        }
    }

    async fn respond_to_event(
        &self,
        input: &CalendarRespondInput,
        reference: BackendEvent,
        expected: Option<&str>,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        let backend = self.require_write(&reference.account_id)?;
        let account = backend.account();
        self.require_calendar_capabilities(&backend, true).await?;
        let _guard = self.write_locks.acquire(&reference.account_id).await?;
        let backend = self.require_write(&reference.account_id)?;
        if let Some(record) =
            self.replay_write("calendar_respond", &input.idempotency_key, input)?
        {
            return Ok((calendar_write_result::existing(record), Vec::new()));
        }
        let mut source = self.account_result(
            &reference.account_id,
            backend.resolve_calendar_source(&reference).await,
        )?;
        source.occurrence_start = reference.occurrence_start;
        let revision = super::calendar_series::revision(&source)?;
        if calendar_prepare::ownership(&source, &account.email) != EventOwnership::Attendee {
            return Err(validation("calendar_respond requires a received meeting"));
        }
        let prepared =
            super::calendar_series::response::prepare(&mut source, input, self.clock.now())?;
        write_preview::verify(
            &response_preview(&reference.account_id, &prepared, input)
                .field("Current revision", revision),
            expected,
        )?;
        let organizer = organizer(&source)?;
        let reply_id = step_client_id(&input.idempotency_key, "reply")?;
        let reply_mime = prepared
            .mutation
            .application
            .response_requested
            .then(|| response_mime(&account.email, &prepared, &organizer, input))
            .transpose()?;
        let begin = self.begin_write(
            &reference.account_id,
            "calendar_respond",
            &input.idempotency_key,
            input,
        )?;
        if !begin.inserted {
            return Ok((calendar_write_result::existing(begin.record), Vec::new()));
        }
        let calendar_id =
            match backend.respond_calendar_item(&source, response_choice(input.response)).await {
                Ok(value) => value,
                Err(error) => return self.calendar_failure(&begin.record, 0, error, None),
            };
        let mut steps = STEP_RESPONSE;
        self.checkpoint_mutation(&begin.record, steps)?;
        let event_ref = Self::journal_after_mutation(
            response_reference(self, source, calendar_id, input.response),
            &begin.record.account_id,
            &begin.record.operation_id,
        )?;
        if let Some(mime) = reply_mime {
            if let Err(error) = backend.send_calendar_message(&reply_id, mime).await {
                return self.calendar_failure(&begin.record, steps, error, event_ref);
            }
            steps |= STEP_REPLY;
            self.checkpoint_mutation(&begin.record, steps)?;
        }
        self.calendar_success(&begin.record, steps, event_ref)
    }

    async fn respond_to_mail(
        &self,
        input: &CalendarRespondInput,
        reference: BackendMail,
        expected: Option<&str>,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        let backend = self.require_write(&reference.account_id)?;
        let account = backend.account();
        self.require_calendar_capabilities(&backend, true).await?;
        let _guard = self.write_locks.acquire(&reference.account_id).await?;
        let backend = self.require_write(&reference.account_id)?;
        let fetched = self.account_result(
            &reference.account_id,
            backend.fetch_mail(&reference.source, 50_000).await,
        )?;
        if input.scope.is_some_and(|scope| scope != crate::model::CalendarScope::Series) {
            return Err(validation("occurrence responses require a calendar occurrence reference"));
        }
        let prepared = calendar_response_prepare::prepare(&fetched, self.clock.now())?;
        write_preview::verify(
            &response_preview(&reference.account_id, &prepared.event, input),
            expected,
        )?;
        let reply_id = step_client_id(&input.idempotency_key, "reply")?;
        let reply_mime =
            response_mime(&account.email, &prepared.event, &prepared.organizer, input)?;
        let begin = self.begin_write(
            &reference.account_id,
            "calendar_respond",
            &input.idempotency_key,
            input,
        )?;
        if !begin.inserted {
            return Ok((calendar_write_result::existing(begin.record), Vec::new()));
        }
        if let Err(error) = backend
            .respond_meeting_request(&reference.source, response_choice(input.response))
            .await
        {
            return self.calendar_failure(&begin.record, 0, error, None);
        }
        let mut steps = STEP_RESPONSE;
        self.checkpoint_mutation(&begin.record, steps)?;
        if let Err(error) = backend.send_calendar_message(&reply_id, reply_mime).await {
            return self.calendar_failure(&begin.record, steps, error, None);
        }
        steps |= STEP_REPLY;
        self.checkpoint_mutation(&begin.record, steps)?;
        self.calendar_success(&begin.record, steps, None)
    }
}

fn response_mime(
    sender: &str,
    event: &calendar_prepare::PreparedEvent,
    organizer: &eas_mail_protocol::CalendarAttendee,
    input: &CalendarRespondInput,
) -> Result<Vec<u8>> {
    required_notification(
        sender,
        event,
        std::slice::from_ref(organizer),
        CalendarMessageMethod::Reply(input.response),
        &input.comment,
    )
}

fn validation(message: &'static str) -> crate::AppError {
    crate::AppError::new(crate::ErrorCode::ValidationFailed, message)
}
