use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::{AccountCapabilities, AppError, ErrorCode, Result, platform};

/// A deliberately separate allowlist schema: never serialize the source diagnostics directly.
#[derive(Debug, Serialize)]
pub(super) struct SupportReport {
    schema_version: u32,
    application_version: &'static str,
    operating_system: &'static str,
    architecture: &'static str,
    pub(super) healthy: bool,
    profile_store_configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<ErrorCode>,
    accounts: Vec<SupportAccount>,
}

#[derive(Debug, Serialize)]
struct SupportAccount {
    status: SupportStatus,
    server_write_permission: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<ErrorCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capabilities: Option<AccountCapabilities>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SupportStatus {
    Ready,
    Disabled,
    Failed,
}

impl SupportReport {
    pub(super) fn from_diagnostics(value: &Value) -> Self {
        let accounts = value
            .get("accounts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(SupportAccount::from_diagnostics)
            .collect::<Vec<_>>();
        let configured = value.get("profile_store").is_some_and(Value::is_object);
        let healthy = configured
            && value.get("config").and_then(Value::as_str) == Some("ok")
            && accounts.iter().any(|account| account.status == SupportStatus::Ready)
            && accounts.iter().all(|account| account.status != SupportStatus::Failed);
        Self {
            schema_version: 1,
            application_version: env!("CARGO_PKG_VERSION"),
            operating_system: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            healthy,
            profile_store_configured: configured,
            error_code: None,
            accounts,
        }
    }

    pub(super) fn failure(code: ErrorCode) -> Self {
        let mut report = Self::from_diagnostics(&Value::Null);
        report.error_code = Some(code);
        report
    }

    pub(super) fn write(&self, path: &Path) -> Result<()> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().map_err(|_| report_error())?.join(path)
        };
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| report_error())?;
        platform::atomic_write_in_existing_directory(&path, &bytes).map_err(|_| report_error())
    }
}

impl SupportAccount {
    fn from_diagnostics(value: &Value) -> Self {
        let status = match value.get("status").and_then(Value::as_str) {
            Some("ok") => SupportStatus::Ready,
            Some("disabled") => SupportStatus::Disabled,
            _ => SupportStatus::Failed,
        };
        let error_code = value
            .get("code")
            .cloned()
            .and_then(|code| serde_json::from_value::<ErrorCode>(code).ok());
        let capabilities = value
            .get("capabilities")
            .filter(|_| status == SupportStatus::Ready)
            .map(|capabilities| AccountCapabilities {
                calendar_availability: capabilities
                    .get("calendar_availability")
                    .and_then(Value::as_str)
                    == Some("available"),
                mail_writes: enabled(capabilities, "mail_writes"),
                personal_calendar_writes: enabled(capabilities, "personal_calendar_writes"),
                meeting_lifecycle: enabled(capabilities, "meeting_lifecycle"),
                auto_reply: enabled(capabilities, "auto_reply"),
                mail_move: enabled(capabilities, "mail_move"),
                mail_properties: enabled(capabilities, "mail_properties"),
            });
        Self { status, server_write_permission: None, error_code, capabilities }
    }
}

fn enabled(value: &Value, field: &str) -> bool {
    value.get(field).and_then(Value::as_bool).unwrap_or(false)
}

fn report_error() -> AppError {
    AppError::new(ErrorCode::StorageError, "cannot save the private support report")
}

#[cfg(test)]
mod tests;
