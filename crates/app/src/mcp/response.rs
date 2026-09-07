mod mutation;
pub(super) use mutation::MutationToolResponse;

use std::borrow::Cow;

use rmcp::handler::server::tool::IntoCallToolResult;
use rmcp::model::{CallToolResponse, CallToolResult};
use schemars::JsonSchema;
use serde::Serialize;

use crate::{ApiResponse, AppError, ErrorCode};

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Typed MCP envelope which preserves runtime failure and advertised schema.
pub(super) struct ToolResponse<T>(pub(super) ApiResponse<T>);

impl<T: JsonSchema> JsonSchema for ToolResponse<T> {
    fn schema_name() -> Cow<'static, str> {
        ApiResponse::<T>::schema_name()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        ApiResponse::<T>::json_schema(generator)
    }
}

impl<T: Serialize + JsonSchema + 'static> IntoCallToolResult for ToolResponse<T> {
    fn into_call_tool_result(self) -> Result<CallToolResponse, rmcp::ErrorData> {
        let bytes = serde_json::to_vec(&self.0).map_err(serialization_error)?;
        let value: serde_json::Value = if bytes.len() > MAX_RESPONSE_BYTES {
            serde_json::to_value(ApiResponse::<()>::failure(
                AppError::new(
                    ErrorCode::ResultTooLarge,
                    "MCP response exceeds 1 MiB; narrow the request",
                )
                .envelope,
            ))
        } else {
            serde_json::from_slice(&bytes)
        }
        .map_err(serialization_error)?;
        let is_error = value.get("error").is_some_and(|error| !error.is_null());
        let mut response = CallToolResult::structured(value);
        response.is_error = Some(is_error);
        Ok(response.into())
    }
}

fn serialization_error(_: serde_json::Error) -> rmcp::ErrorData {
    rmcp::ErrorData::internal_error("Cannot serialize tool response", None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_failures_are_mcp_tool_errors_with_structured_content() -> anyhow::Result<()> {
        let response = ToolResponse::<()>(ApiResponse::failure(
            AppError::new(ErrorCode::OutcomeUnknown, "outcome unknown")
                .operation("fixture-id")
                .envelope,
        ))
        .into_call_tool_result()?;
        let value = completed(response)?;
        assert_eq!(value.get("isError"), Some(&serde_json::Value::Bool(true)));
        assert_eq!(
            value.pointer("/structuredContent/error/code").and_then(serde_json::Value::as_str),
            Some("OUTCOME_UNKNOWN")
        );
        Ok(())
    }

    #[test]
    fn successful_and_oversized_results_preserve_envelope_semantics() -> anyhow::Result<()> {
        for (data, expected) in [("ok".to_owned(), false), ("x".repeat(MAX_RESPONSE_BYTES), true)] {
            let response =
                ToolResponse(ApiResponse::success(data, Vec::new())).into_call_tool_result()?;
            let value = completed(response)?;
            assert_eq!(value.get("isError"), Some(&serde_json::Value::Bool(expected)));
            if expected {
                assert_eq!(
                    value
                        .pointer("/structuredContent/error/code")
                        .and_then(serde_json::Value::as_str),
                    Some("RESULT_TOO_LARGE")
                );
            }
        }
        Ok(())
    }

    fn completed(response: CallToolResponse) -> anyhow::Result<serde_json::Value> {
        let CallToolResponse::Complete(result) = response else {
            anyhow::bail!("expected a completed tool response");
        };
        Ok(serde_json::to_value(result)?)
    }
}
