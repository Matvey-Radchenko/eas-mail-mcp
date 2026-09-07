use serde::Serialize;

use super::Runtime;
use super::mail_write_preview::{forward, mark_read_preview, message_preview, send_message};
use super::outgoing::{reply_message, validate_message};
use super::outgoing_attachments;
use super::write_preview;
use crate::journal::payload_fingerprint;
use crate::model::{
    ApiResponse, MailForwardInput, MailReplyInput, MailSendInput, MarkReadInput, OperationResult,
    OperationState,
};
use crate::{AppError, ErrorCode, JournalBegin, JournalRecord, OperationStatus, Result};

impl Runtime {
    /// Changes read state through an idempotent EAS Sync mutation.
    pub async fn mail_mark_read(&self, input: MarkReadInput) -> ApiResponse<OperationResult> {
        Self::response(self.mark_read_result(input, None).await)
    }

    /// Sends one plain-text message with durable idempotency metadata.
    pub async fn mail_send(&self, input: MailSendInput) -> ApiResponse<OperationResult> {
        Self::response(self.send_result(input, None).await)
    }

    /// Replies to a message selected by a portable reference.
    pub async fn mail_reply(&self, input: MailReplyInput) -> ApiResponse<OperationResult> {
        Self::response(self.reply_result(input, None).await)
    }

    /// Forwards a message selected by a portable reference.
    pub async fn mail_forward(&self, input: MailForwardInput) -> ApiResponse<OperationResult> {
        Self::response(self.forward_result(input, None).await)
    }

    pub(crate) async fn commit_cli_mail_mark_read(
        &self,
        input: MarkReadInput,
        expected: &str,
    ) -> ApiResponse<OperationResult> {
        Self::response(self.mark_read_result(input, Some(expected)).await)
    }

    pub(crate) async fn commit_cli_mail_send(
        &self,
        input: MailSendInput,
        expected: &str,
    ) -> ApiResponse<OperationResult> {
        Self::response(self.send_result(input, Some(expected)).await)
    }

    pub(crate) async fn commit_cli_mail_reply(
        &self,
        input: MailReplyInput,
        expected: &str,
    ) -> ApiResponse<OperationResult> {
        Self::response(self.reply_result(input, Some(expected)).await)
    }

    pub(crate) async fn commit_cli_mail_forward(
        &self,
        input: MailForwardInput,
        expected: &str,
    ) -> ApiResponse<OperationResult> {
        Self::response(self.forward_result(input, Some(expected)).await)
    }

    async fn mark_read_result(
        &self,
        input: MarkReadInput,
        expected: Option<&str>,
    ) -> Result<(OperationResult, Vec<crate::Warning>)> {
        if let Some(record) = self.replay_write("mail_mark_read", &input.idempotency_key, &input)? {
            return Ok((existing_result(record), Vec::new()));
        }
        let mail = self.references.mail(&input.mail_ref)?;
        self.require_write(&mail.account_id)?;
        let _guard = self.write_locks.acquire(&mail.account_id).await?;
        let backend = self.require_write(&mail.account_id)?;
        let fetched =
            self.account_result(&mail.account_id, backend.fetch_mail(&mail.source, 1).await)?;
        write_preview::verify(&mark_read_preview(&fetched, input.is_read), expected)?;
        self.account_result(
            &mail.account_id,
            backend.check_mail_property_ready(&fetched.source).await,
        )?;
        let begin =
            self.begin_write(&mail.account_id, "mail_mark_read", &input.idempotency_key, &input)?;
        if !begin.inserted {
            return Ok((existing_result(begin.record), Vec::new()));
        }
        let result = backend.mark_read(&mail.source, input.is_read).await;
        self.finish_write(&mail.account_id, &begin.record.operation_id, result)
            .map(|value| (value, Vec::new()))
    }

    async fn send_result(
        &self,
        input: MailSendInput,
        expected: Option<&str>,
    ) -> Result<(OperationResult, Vec<crate::Warning>)> {
        let attachments = outgoing_attachments::prepare(&input.attachments)?;
        let payload = outgoing_attachments::payload(&input, &attachments);
        if let Some(record) = self.replay_write("mail_send", &input.idempotency_key, &payload)? {
            return Ok((existing_result(record), Vec::new()));
        }
        let mut message = send_message(&input);
        message.attachments = attachments;
        validate_message(&message)?;
        self.require_write(&input.account_id)?;
        let _guard = self.write_locks.acquire(&input.account_id).await?;
        let backend = self.require_write(&input.account_id)?;
        write_preview::verify(
            &message_preview("mail_send", &input.account_id, &message),
            expected,
        )?;
        let begin =
            self.begin_write(&input.account_id, "mail_send", &input.idempotency_key, &payload)?;
        if !begin.inserted {
            return Ok((existing_result(begin.record), Vec::new()));
        }
        let result = backend.send(&begin.record.client_id, &message).await;
        self.finish_write(&input.account_id, &begin.record.operation_id, result)
            .map(|value| (value, Vec::new()))
    }

    async fn reply_result(
        &self,
        input: MailReplyInput,
        expected: Option<&str>,
    ) -> Result<(OperationResult, Vec<crate::Warning>)> {
        let attachments = outgoing_attachments::prepare(&input.attachments)?;
        let payload = outgoing_attachments::payload(&input, &attachments);
        if let Some(record) = self.replay_write("mail_reply", &input.idempotency_key, &payload)? {
            return Ok((existing_result(record), Vec::new()));
        }
        let reference = self.references.mail(&input.mail_ref)?;
        self.require_write(&reference.account_id)?;
        let _guard = self.write_locks.acquire(&reference.account_id).await?;
        let backend = self.require_write(&reference.account_id)?;
        let mail = self.account_result(
            &reference.account_id,
            backend.fetch_mail(&reference.source, 1).await,
        )?;
        let mut message = reply_message(&mail, &backend.account().email, &input)?;
        message.attachments = attachments;
        validate_message(&message)?;
        write_preview::verify(
            &message_preview("mail_reply", &reference.account_id, &message),
            expected,
        )?;
        let begin = self.begin_write(
            &reference.account_id,
            "mail_reply",
            &input.idempotency_key,
            &payload,
        )?;
        if !begin.inserted {
            return Ok((existing_result(begin.record), Vec::new()));
        }
        let result = backend.reply(&begin.record.client_id, &reference.source, &message).await;
        self.finish_write(&reference.account_id, &begin.record.operation_id, result)
            .map(|value| (value, Vec::new()))
    }

    async fn forward_result(
        &self,
        input: MailForwardInput,
        expected: Option<&str>,
    ) -> Result<(OperationResult, Vec<crate::Warning>)> {
        let attachments = outgoing_attachments::prepare(&input.attachments)?;
        let payload = outgoing_attachments::payload(&input, &attachments);
        if let Some(record) = self.replay_write("mail_forward", &input.idempotency_key, &payload)? {
            return Ok((existing_result(record), Vec::new()));
        }
        let reference = self.references.mail(&input.mail_ref)?;
        self.require_write(&reference.account_id)?;
        let _guard = self.write_locks.acquire(&reference.account_id).await?;
        let backend = self.require_write(&reference.account_id)?;
        let mail = self.account_result(
            &reference.account_id,
            backend.fetch_mail(&reference.source, 1).await,
        )?;
        let mut message = forward(&mail, &input);
        message.attachments = attachments;
        validate_message(&message)?;
        write_preview::verify(
            &message_preview("mail_forward", &reference.account_id, &message),
            expected,
        )?;
        let begin = self.begin_write(
            &reference.account_id,
            "mail_forward",
            &input.idempotency_key,
            &payload,
        )?;
        if !begin.inserted {
            return Ok((existing_result(begin.record), Vec::new()));
        }
        let result = backend.forward(&begin.record.client_id, &reference.source, &message).await;
        self.finish_write(&reference.account_id, &begin.record.operation_id, result)
            .map(|value| (value, Vec::new()))
    }

    pub(super) fn begin_write<T: Serialize>(
        &self,
        account_id: &str,
        kind: &str,
        operation_id: &str,
        payload: &T,
    ) -> Result<JournalBegin> {
        let record = self.write_record(account_id, kind, operation_id, payload)?;
        self.journal.begin(&record)
    }

    pub(super) fn replay_write<T: Serialize>(
        &self,
        kind: &str,
        operation_id: &str,
        payload: &T,
    ) -> Result<Option<JournalRecord>> {
        let candidate = self.write_record("", kind, operation_id, payload)?;
        let Some(existing) = self.journal.lookup(&candidate.operation_id)? else {
            return Ok(None);
        };
        if existing.kind != candidate.kind || existing.payload_hmac != candidate.payload_hmac {
            return Err(AppError::new(
                ErrorCode::IdempotencyConflict,
                "idempotency key was already used for different input",
            ));
        }
        Ok(Some(existing))
    }

    fn write_record<T: Serialize>(
        &self,
        account_id: &str,
        kind: &str,
        operation_id: &str,
        payload: &T,
    ) -> Result<JournalRecord> {
        let parsed = uuid::Uuid::parse_str(operation_id).map_err(|_| {
            AppError::new(ErrorCode::ValidationFailed, "idempotency_key must be a UUID")
        })?;
        let canonical = serde_json::to_vec(payload).map_err(|_| {
            AppError::new(ErrorCode::ValidationFailed, "cannot canonicalize operation input")
        })?;
        Ok(JournalRecord {
            operation_id: parsed.to_string(),
            account_id: account_id.to_owned(),
            kind: kind.to_owned(),
            payload_hmac: payload_fingerprint(&self.hmac_key, &canonical)?,
            client_id: parsed.to_string(),
            status: OperationStatus::Pending,
            completed_steps: 0,
        })
    }
}

pub(super) fn existing_result(record: JournalRecord) -> OperationResult {
    let (status, message) = match record.status {
        OperationStatus::Succeeded => {
            (OperationState::Succeeded, "the prior operation was confirmed")
        }
        OperationStatus::Failed => (OperationState::Failed, "the prior operation failed safely"),
        OperationStatus::Partial => {
            (OperationState::Failed, "the prior operation completed only some Calendar steps")
        }
        OperationStatus::Pending | OperationStatus::Unknown => {
            (OperationState::Unknown, "the prior operation outcome is unknown")
        }
    };
    OperationResult { operation_id: record.operation_id, status, message: message.into() }
}
