use serde::Serialize;
use sha2::{Digest as _, Sha256};

use crate::{AppError, ErrorCode, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct WritePreview {
    operation: &'static str,
    account_id: String,
    fields: Vec<PreviewField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct PreviewField {
    label: &'static str,
    value: String,
}

pub(crate) enum PreparedWrite<T> {
    Replay(T),
    Ready(WritePreview),
}

impl WritePreview {
    pub(super) fn new(operation: &'static str, account_id: String) -> Self {
        Self { operation, account_id, fields: Vec::new() }
    }

    pub(super) fn field(mut self, label: &'static str, value: impl Into<String>) -> Self {
        self.fields.push(PreviewField { label, value: value.into() });
        self
    }

    pub(crate) fn fingerprint(&self) -> Result<String> {
        let bytes = serde_json::to_vec(self).map_err(|_| {
            AppError::new(ErrorCode::ProtocolError, "cannot serialize write preview")
        })?;
        let digest = Sha256::digest(bytes);
        Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    pub(super) fn append(mut self, other: Self) -> Self {
        self.fields.extend(other.fields);
        self
    }

    pub(crate) fn render(&self) -> String {
        let mut lines = vec![
            format!("Operation: {}", literal(self.operation)),
            format!("Account: {}", literal(&self.account_id)),
        ];
        lines.extend(
            self.fields.iter().map(|field| format!("{}: {}", field.label, literal(&field.value))),
        );
        lines.join("\n")
    }
}

pub(super) fn verify(preview: &WritePreview, expected: Option<&str>) -> Result<()> {
    if let Some(expected) = expected
        && preview.fingerprint()? != expected
    {
        return Err(AppError::new(
            ErrorCode::SyncStale,
            "the write target changed after preview; review the operation again",
        )
        .retryable());
    }
    Ok(())
}

fn literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"unavailable\"".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_is_literal_complete_and_content_sensitive() -> anyhow::Result<()> {
        let first = WritePreview::new("mail_send", "account".into())
            .field("Body", "line one\n\u{1b}[31mline two");
        let second =
            WritePreview::new("mail_send", "account".into()).field("Body", "line one\nline two");
        let rendered = first.render();
        assert!(rendered.contains("Body: \"line one\\n\\u001b[31mline two\""));
        assert!(!rendered.contains('\u{1b}'));
        assert_ne!(first.fingerprint()?, second.fingerprint()?);
        assert!(verify(&first, Some(&first.fingerprint()?)).is_ok());
        let error = verify(&first, Some(&second.fingerprint()?))
            .err()
            .ok_or_else(|| anyhow::anyhow!("changed preview unexpectedly passed verification"))?;
        assert_eq!(error.envelope.code, ErrorCode::SyncStale);
        Ok(())
    }
}
