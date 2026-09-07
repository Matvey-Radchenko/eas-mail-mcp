use super::write_preview::{PreparedWrite, WritePreview};
use crate::model::{
    ApiResponse, MailBatchData, MailBatchEntry, MailBatchInput, MailGetInput, MailGetManyData,
    MailGetManyEntry, MailGetManyInput, Warning,
};
use crate::{AppError, ErrorCode, ErrorEnvelope, Result, Runtime};
use std::collections::{BTreeMap, BTreeSet};

impl Runtime {
    /// Fetches up to 20 messages with individual errors and a shared text budget.
    pub async fn mail_get_many(&self, input: MailGetManyInput) -> ApiResponse<MailGetManyData> {
        Self::response(self.get_many(input).await)
    }

    async fn get_many(&self, input: MailGetManyInput) -> Result<(MailGetManyData, Vec<Warning>)> {
        validate_count(input.mail_refs.len())?;
        let mut unique = BTreeSet::new();
        let mut warnings = Vec::new();
        let mut items = Vec::new();
        let limit = input.body_limit.unwrap_or(12_000);
        let mut remaining = input.total_body_limit.unwrap_or(100_000);
        if limit == 0 || limit > 50_000 || remaining == 0 || remaining > 100_000 {
            return Err(validation(
                "body limits must be positive; per-message maximum 50,000, shared maximum 100,000",
            ));
        }
        for reference in &input.mail_refs {
            let mail = self.references.mail(reference)?;
            if !unique.insert(identity(&mail.account_id, &mail.source)) {
                return Err(validation("a message may appear only once"));
            }
        }
        let mut bodies_truncated = false;
        for mail_ref in input.mail_refs {
            let response = self
                .mail_get(MailGetInput {
                    mail_ref: mail_ref.clone(),
                    body_limit: Some(limit.min(remaining.max(1))),
                })
                .await;
            if let Some(error) = &response.error {
                warnings.push(warning(error));
            }
            let mut mail = response.data;
            if let Some(mail) = &mut mail {
                if remaining == 0 {
                    mail.body.clear();
                    mail.summary.preview.clear();
                    mail.body_truncated = true;
                }
                remaining = remaining.saturating_sub(mail.body.chars().count() as u32);
                bodies_truncated |= mail.body_truncated;
            }
            items.push(MailGetManyEntry { mail_ref, mail, error: response.error });
        }
        Ok((MailGetManyData { items, bodies_truncated }, warnings))
    }

    /// Runs an ordered bounded batch; ambiguous outcomes stop further writes for that account.
    pub async fn mail_batch(&self, input: MailBatchInput) -> ApiResponse<MailBatchData> {
        Self::response(self.run_batch(input, None).await)
    }

    pub(crate) async fn prepare_cli_mail_batch(
        &self,
        input: &MailBatchInput,
    ) -> Result<WritePreview> {
        self.prepare_batch(input).await.map(|(preview, _)| preview)
    }

    async fn prepare_batch(
        &self,
        input: &MailBatchInput,
    ) -> Result<(WritePreview, Vec<Option<String>>)> {
        let failures = self.validate_batch(input).await?;
        let mut preview = WritePreview::new("mail_batch", "per-entry accounts".into());
        let mut fingerprints = Vec::new();
        for (entry, failure) in input.items.iter().zip(failures) {
            if let Some(error) = failure {
                return Err(error);
            }
            preview = match self.prepare_cli_mail_mutation(entry).await? {
                PreparedWrite::Ready(item) => {
                    fingerprints.push(Some(item.fingerprint()?));
                    preview.append(item)
                }
                PreparedWrite::Replay(result) => {
                    fingerprints.push(None);
                    preview.field("Replay UUID", result.operation_id)
                }
            };
        }
        Ok((preview, fingerprints))
    }

    pub(crate) async fn commit_cli_mail_batch(
        &self,
        input: MailBatchInput,
        expected: &str,
    ) -> ApiResponse<MailBatchData> {
        let (preview, fingerprints) = match self.prepare_batch(&input).await {
            Ok(preview) => preview,
            Err(error) => return ApiResponse::failure(error.envelope),
        };
        if let Err(error) = super::write_preview::verify(&preview, Some(expected)) {
            return ApiResponse::failure(error.envelope);
        }
        // Every fingerprint belongs to the exact preview just compared with the approval.
        // Each entry checks it again under the account write lock before beginning a write.
        Self::response(self.run_batch(input, Some(fingerprints)).await)
    }

    async fn run_batch(
        &self,
        input: MailBatchInput,
        expected: Option<Vec<Option<String>>>,
    ) -> Result<(MailBatchData, Vec<Warning>)> {
        let failures = self.validate_batch(&input).await?;
        let mut blocked: BTreeMap<String, String> = BTreeMap::new();
        let mut items = Vec::new();
        let mut warnings = Vec::new();
        for (index, (entry, failure)) in input.items.into_iter().zip(failures).enumerate() {
            let account = self.references.mail(&entry.mail_ref)?.account_id;
            let stopped = blocked.get(&account);
            let skipped = stopped.is_some();
            let response = if let Some(operation_id) = stopped {
                ApiResponse::failure(AppError::new(ErrorCode::OutcomeUnknown, "entry skipped because a previous operation for this account has an unknown outcome").account(&account).operation(operation_id).envelope)
            } else if let Some(error) = failure {
                ApiResponse::failure(error.account(&account).envelope)
            } else {
                let fingerprint = expected
                    .as_ref()
                    .and_then(|values| values.get(index))
                    .and_then(Option::as_deref);
                self.mail_mutation(entry.clone(), fingerprint).await
            };
            if let Some(error) = &response.error {
                warnings.push(warning(error));
                if error.code == ErrorCode::OutcomeUnknown {
                    blocked.entry(account).or_insert_with(|| entry.idempotency_key.clone());
                }
            }
            items.push(MailBatchEntry {
                mail_ref: entry.mail_ref,
                operation_id: entry.idempotency_key,
                result: response.data,
                error: response.error,
                skipped,
            });
        }
        Ok((MailBatchData { items }, warnings))
    }

    async fn validate_batch(&self, input: &MailBatchInput) -> Result<Vec<Option<AppError>>> {
        validate_count(input.items.len())?;
        let mut uuids = BTreeSet::new();
        let mut identities = BTreeSet::new();
        let mut failures = Vec::new();
        for entry in &input.items {
            super::mail_mutation::validate(entry)?;
            let uuid = uuid::Uuid::parse_str(&entry.idempotency_key)
                .map_err(|_| validation("invalid operation UUID"))?;
            if !uuids.insert(uuid) {
                return Err(validation("each batch entry requires a distinct UUID"));
            }
            let mail = self.references.mail(&entry.mail_ref)?;
            let mut source = mail.source;
            let mut failure = None;
            if self.journal.lookup(&uuid.to_string())?.is_none() {
                match self.require_write(&mail.account_id) {
                    Ok(backend) => match self.account_result(
                        &mail.account_id,
                        backend.resolve_mail_source(&source).await,
                    ) {
                        Ok(resolved) => source = resolved.source,
                        Err(error) => failure = Some(error),
                    },
                    Err(error) => failure = Some(error),
                }
            }
            if !identities.insert(identity(&mail.account_id, &source)) {
                return Err(validation("a message may appear only once in a batch"));
            }
            failures.push(failure);
        }
        Ok(failures)
    }
}

fn validate_count(count: usize) -> Result<()> {
    if count == 0 || count > 20 {
        Err(validation("request requires 1–20 messages"))
    } else {
        Ok(())
    }
}
fn validation(message: &str) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}
fn identity(account: &str, source: &crate::backend::MailSource) -> String {
    format!("{account}:{source:?}")
}
fn warning(error: &ErrorEnvelope) -> Warning {
    Warning {
        account_id: error.account_id.clone().unwrap_or_default(),
        code: error.code.as_str().into(),
        message: error.message.clone(),
        retryable: error.retryable,
        remediation: error.remediation.clone(),
        operation_id: error.operation_id.clone(),
        retry_after_seconds: error.retry_after_seconds,
    }
}
