use std::borrow::Cow;

use rmcp::handler::server::tool::IntoCallToolResult;
use rmcp::model::CallToolResponse;
use schemars::JsonSchema;
use serde::Serialize;

use super::ToolResponse;
use crate::model::{
    AutoReplyOperationResult, AutoReplyOperationState, CalendarOperationResult,
    CalendarOperationState, MailMutationResult, OperationResult, OperationState,
};
use crate::{ApiResponse, Warning};

/// Single writes distinguish historic rejection/uncertainty from read-only metadata.
pub(in crate::mcp) struct MutationToolResponse<T>(pub(in crate::mcp) ApiResponse<T>);

pub(in crate::mcp) trait MutationOutcome {
    fn unsuccessful(&self) -> bool;
    fn partial_operation_id(&self) -> Option<&str> {
        None
    }
}

impl<T: JsonSchema> JsonSchema for MutationToolResponse<T> {
    fn schema_name() -> Cow<'static, str> {
        ApiResponse::<T>::schema_name()
    }
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        ApiResponse::<T>::json_schema(generator)
    }
}

impl<T: Serialize + JsonSchema + MutationOutcome + 'static> IntoCallToolResult
    for MutationToolResponse<T>
{
    fn into_call_tool_result(mut self) -> Result<CallToolResponse, rmcp::ErrorData> {
        let unsuccessful = self.0.data.as_ref().is_some_and(MutationOutcome::unsuccessful);
        if let Some(operation_id) =
            self.0.data.as_ref().and_then(MutationOutcome::partial_operation_id)
            && self.0.warnings.is_empty()
        {
            self.0.warnings.push(Warning {
                account_id: String::new(),
                code: "PARTIAL_WRITE".into(),
                message: "Only part of the operation was confirmed".into(),
                retryable: false,
                remediation: Some(
                    "Inspect the operation and Exchange state; do not retry with a new UUID".into(),
                ),
                operation_id: Some(operation_id.into()),
                retry_after_seconds: None,
            });
        }
        let mut response = ToolResponse(self.0).into_call_tool_result()?;
        if unsuccessful && let CallToolResponse::Complete(result) = &mut response {
            result.is_error = Some(true);
        }
        Ok(response)
    }
}

impl MutationOutcome for OperationResult {
    fn unsuccessful(&self) -> bool {
        matches!(self.status, OperationState::Failed | OperationState::Unknown)
    }
}

impl MutationOutcome for MailMutationResult {
    fn unsuccessful(&self) -> bool {
        matches!(self.status, OperationState::Failed | OperationState::Unknown)
    }
}

impl MutationOutcome for CalendarOperationResult {
    fn unsuccessful(&self) -> bool {
        matches!(self.status, CalendarOperationState::Failed | CalendarOperationState::Unknown)
    }
    fn partial_operation_id(&self) -> Option<&str> {
        matches!(self.status, CalendarOperationState::Partial).then_some(self.operation_id.as_str())
    }
}

impl MutationOutcome for AutoReplyOperationResult {
    fn unsuccessful(&self) -> bool {
        matches!(self.status, AutoReplyOperationState::Failed | AutoReplyOperationState::Unknown)
    }
    fn partial_operation_id(&self) -> Option<&str> {
        matches!(self.status, AutoReplyOperationState::Partial)
            .then_some(self.operation_id.as_str())
    }
}
