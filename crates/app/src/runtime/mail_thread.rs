use eas_mail_protocol::{MailSearchQuery, Patch};

use super::mail_search::search_candidates;
use crate::model::{MailGetThreadInput, MailSearchFilters, MailThreadData, Warning};
use crate::sanitize::limit;
use crate::{ApiResponse, AppError, ErrorCode, Result, Runtime};

impl Runtime {
    /// Reads one bounded Exchange conversation without synchronizing any mail collection.
    pub async fn mail_get_thread(&self, input: MailGetThreadInput) -> ApiResponse<MailThreadData> {
        Self::response(self.mail_thread_result(input).await)
    }

    async fn mail_thread_result(
        &self,
        input: MailGetThreadInput,
    ) -> Result<(MailThreadData, Vec<Warning>)> {
        let result_limit = limit(input.limit.map(u32::from), 20, 100)?;
        let body_limit = limit(input.body_limit, 12_000, 50_000)?;
        let mut remaining = limit(input.total_body_limit, 100_000, 100_000)?;
        let reference = self.references.mail(&input.mail_ref)?;
        let backend = self.backend(&reference.account_id)?;
        let seed = self.account_result(
            &reference.account_id,
            backend.fetch_mail(&reference.source, 1).await,
        )?;
        let conversation_id = conversation_id(&seed.fields.conversation_id)?;
        let query =
            MailSearchQuery { conversation_id: Some(conversation_id), ..Default::default() };
        let mut found = self.account_result(
            &reference.account_id,
            search_candidates(backend.as_ref(), &query, &MailSearchFilters::default()).await,
        )?;
        if found.items.is_empty() {
            return Err(unavailable("Exchange did not return the referenced conversation"));
        }
        found.items.sort_by(|left, right| {
            receive_time(&left.fields)
                .cmp(&receive_time(&right.fields))
                .then_with(|| index(&left.fields).cmp(index(&right.fields)))
        });
        let mut results_truncated =
            !found.coverage.candidates_complete || found.items.len() > result_limit;
        found.items.truncate(result_limit);
        let mut items = Vec::new();
        let mut warnings = Vec::new();
        let mut failure = None;
        for candidate in found.items {
            let fetched = if remaining == 0 {
                Ok(candidate)
            } else {
                self.account_result(
                    &reference.account_id,
                    backend.fetch_mail(&candidate.source, body_limit.min(remaining)).await,
                )
            };
            let mail = match fetched {
                Ok(mail) => mail,
                Err(error) => {
                    warnings.push(Warning {
                        account_id: reference.account_id.clone(),
                        code: error.envelope.code.as_str().into(),
                        message: "One conversation message could not be read".into(),
                        retryable: error.envelope.retryable,
                        remediation: error.envelope.remediation.clone(),
                        operation_id: error.envelope.operation_id.clone(),
                        retry_after_seconds: error.envelope.retry_after_seconds,
                    });
                    failure = Some(error);
                    results_truncated = true;
                    continue;
                }
            };
            if mail.fields.conversation_id != seed.fields.conversation_id {
                return Err(unavailable(
                    "Exchange did not verify every message's conversation identifier",
                ));
            }
            let mail_ref = self.references.insert_mail(mail.clone())?;
            let mut detail = self.mail_detail(mail_ref, &mail, body_limit.min(remaining));
            if remaining == 0 {
                detail.body_truncated = true;
            }
            remaining = remaining.saturating_sub(detail.body.chars().count());
            items.push(detail);
        }
        if items.is_empty()
            && let Some(error) = failure
        {
            return Err(error);
        }
        let bodies_truncated = items.iter().any(|item| item.body_truncated);
        Ok((
            MailThreadData { items, results_truncated, bodies_truncated, coverage: found.coverage },
            warnings,
        ))
    }
}

fn conversation_id(value: &Patch<Vec<u8>>) -> Result<Vec<u8>> {
    let Patch::Value(value) = value else {
        return Err(unavailable("Exchange did not provide a conversation identifier"));
    };
    if value.len() != 16 {
        return Err(unavailable("Exchange returned an unsupported conversation identifier"));
    }
    Ok(value.clone())
}

fn receive_time(fields: &eas_mail_protocol::MailFields) -> Option<chrono::DateTime<chrono::Utc>> {
    match fields.received_at {
        Patch::Value(value) => value,
        Patch::Missing => None,
    }
}

fn index(fields: &eas_mail_protocol::MailFields) -> &[u8] {
    match &fields.conversation_index {
        Patch::Value(value) => value,
        Patch::Missing => &[],
    }
}

fn unavailable(message: &'static str) -> AppError {
    AppError::new(ErrorCode::FeatureUnavailable, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_identifier_stays_opaque_without_text_or_byte_order_conversion() -> Result<()> {
        let bytes = vec![
            0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ];
        assert_eq!(conversation_id(&Patch::Value(bytes.clone()))?, bytes);
        assert!(conversation_id(&Patch::Missing).is_err());
        assert!(conversation_id(&Patch::Value(vec![0; 8])).is_err());
        Ok(())
    }
}
