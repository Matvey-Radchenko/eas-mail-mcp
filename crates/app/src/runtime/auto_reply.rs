use super::Runtime;
use super::auto_reply_support::{existing, matches, observed, preview, requested};
use super::write_preview::{self, PreparedWrite};
use crate::{
    ApiResponse, AutoReplyGetInput, AutoReplyOperationResult, AutoReplyOperationState,
    AutoReplySetInput, AutoReplySettings, ErrorCode, OperationStatus, Result, Warning,
};

const KIND: &str = "mail_set_auto_reply";

impl Runtime {
    /// Reads fresh out-of-office settings for one explicitly selected account.
    pub async fn mail_get_auto_reply(
        &self,
        input: AutoReplyGetInput,
    ) -> ApiResponse<AutoReplySettings> {
        Self::response(self.auto_reply_get_result(input).await)
    }

    /// Updates out-of-office settings once and verifies the effective settings with a fresh read.
    pub async fn mail_set_auto_reply(
        &self,
        input: AutoReplySetInput,
    ) -> ApiResponse<AutoReplyOperationResult> {
        Self::response(self.auto_reply_set_result(input, None).await)
    }

    pub(crate) async fn prepare_cli_auto_reply(
        &self,
        input: &AutoReplySetInput,
    ) -> Result<PreparedWrite<ApiResponse<AutoReplyOperationResult>>> {
        if let Some(record) = self.replay_write(KIND, &input.idempotency_key, input)? {
            let (result, warnings) = with_warnings(existing(record), &input.account_id);
            return Ok(PreparedWrite::Replay(ApiResponse::success(result, warnings)));
        }
        let _ = requested(input, self.clock.now())?;
        let backend = self.require_write(&input.account_id)?;
        let current = self.account_result(&input.account_id, backend.get_auto_reply().await)?;
        Ok(PreparedWrite::Ready(preview(input, &current)))
    }

    pub(crate) async fn commit_cli_auto_reply(
        &self,
        input: AutoReplySetInput,
        expected: &str,
    ) -> ApiResponse<AutoReplyOperationResult> {
        Self::response(self.auto_reply_set_result(input, Some(expected)).await)
    }

    async fn auto_reply_get_result(
        &self,
        input: AutoReplyGetInput,
    ) -> Result<(AutoReplySettings, Vec<Warning>)> {
        let backend = self.backend(&input.account_id)?;
        let settings = self.account_result(&input.account_id, backend.get_auto_reply().await)?;
        Ok((observed(&input.account_id, &settings), Vec::new()))
    }

    async fn auto_reply_set_result(
        &self,
        input: AutoReplySetInput,
        expected: Option<&str>,
    ) -> Result<(AutoReplyOperationResult, Vec<Warning>)> {
        if let Some(record) = self.replay_write(KIND, &input.idempotency_key, &input)? {
            return Ok(with_warnings(existing(record), &input.account_id));
        }
        let settings = requested(&input, self.clock.now())?;
        self.require_write(&input.account_id)?;
        let _guard = self.write_locks.acquire(&input.account_id).await?;
        let backend = self.require_write(&input.account_id)?;
        if expected.is_some() {
            let current = self.account_result(&input.account_id, backend.get_auto_reply().await)?;
            write_preview::verify(&preview(&input, &current), expected)?;
        }
        let begin = self.begin_write(&input.account_id, KIND, &input.idempotency_key, &input)?;
        if !begin.inserted {
            return Ok(with_warnings(existing(begin.record), &input.account_id));
        }
        let operation_id = &begin.record.operation_id;
        if let Err(error) = backend.set_auto_reply(&settings).await {
            if error.envelope.code == ErrorCode::RemoteWipe {
                Self::journal_after_mutation(
                    self.purge_account(&input.account_id),
                    &input.account_id,
                    operation_id,
                )?;
            } else {
                let status = if error.envelope.code == ErrorCode::OutcomeUnknown {
                    OperationStatus::Unknown
                } else {
                    OperationStatus::Failed
                };
                Self::journal_after_mutation(
                    self.journal.finish(operation_id, status, 0),
                    &input.account_id,
                    operation_id,
                )?;
            }
            return Err(error.operation(operation_id));
        }
        // Persist acknowledgement before the verification read so a crash cannot trigger a resend.
        Self::journal_after_mutation(
            self.journal.finish(operation_id, OperationStatus::Partial, 1),
            &input.account_id,
            operation_id,
        )?;
        let verified = backend.get_auto_reply().await;
        let (status, message, actual) = match verified {
            Ok(actual) if matches(&settings, &actual) => {
                Self::journal_after_mutation(
                    self.journal.finish(operation_id, OperationStatus::Succeeded, 1),
                    &input.account_id,
                    operation_id,
                )?;
                (
                    AutoReplyOperationState::Succeeded,
                    "Exchange confirmed and verified automatic-reply settings",
                    Some(actual),
                )
            }
            Ok(actual) => (
                AutoReplyOperationState::Partial,
                "Exchange accepted the update but effective settings differ; review the returned settings",
                Some(actual),
            ),
            Err(error) if error.envelope.code == ErrorCode::RemoteWipe => {
                Self::journal_after_mutation(
                    self.purge_account(&input.account_id),
                    &input.account_id,
                    operation_id,
                )?;
                return Err(error.operation(operation_id));
            }
            Err(_) => (
                AutoReplyOperationState::Partial,
                "Exchange accepted the update but verification failed; read current settings before making another update",
                None,
            ),
        };
        Ok(with_warnings(
            AutoReplyOperationResult {
                operation_id: operation_id.clone(),
                status,
                message: message.into(),
                settings: actual.map(|value| observed(&input.account_id, &value)),
            },
            &input.account_id,
        ))
    }
}

fn with_warnings(
    result: AutoReplyOperationResult,
    account_id: &str,
) -> (AutoReplyOperationResult, Vec<Warning>) {
    let warnings = if result.status == AutoReplyOperationState::Partial {
        vec![Warning {
            account_id: account_id.into(),
            code: "PARTIAL_WRITE".into(),
            message: "Exchange accepted the automatic-reply update, but complete verification was not confirmed".into(),
            retryable: false,
            remediation: Some("Read current settings before making another update; do not retry with a new UUID".into()),
            operation_id: Some(result.operation_id.clone()),
            retry_after_seconds: None,
        }]
    } else {
        Vec::new()
    };
    (result, warnings)
}
