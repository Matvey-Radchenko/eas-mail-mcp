use std::fmt;

use eas_mail_mcp::ApiResponse;

/// A write whose outcome must be inspected before any follow-up mutation.
#[derive(Debug)]
struct IncompleteWrite {
    operation: String,
    status: &'static str,
    operation_id: Option<String>,
}

impl fmt::Display for IncompleteWrite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} returned {}; operation_id={:?}; no cleanup or further writes attempted",
            self.operation, self.status, self.operation_id
        )
    }
}

impl std::error::Error for IncompleteWrite {}

pub fn incomplete(operation: &str, status: &'static str, id: Option<&str>) -> anyhow::Error {
    IncompleteWrite { operation: operation.into(), status, operation_id: id.map(str::to_owned) }
        .into()
}

pub fn must_stop(error: &anyhow::Error) -> bool {
    error.is::<IncompleteWrite>()
}

pub fn check_warnings<T>(response: &ApiResponse<T>, operation: &str) -> anyhow::Result<()> {
    for warning in &response.warnings {
        let state = match warning.code.as_str() {
            "PARTIAL_WRITE" => "Partial",
            "OUTCOME_UNKNOWN" => "Unknown",
            _ => continue,
        };
        return Err(incomplete(operation, state, warning.operation_id.as_deref()));
    }
    Ok(())
}
