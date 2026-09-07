use anyhow::Result;
use serde_json::Value;

use super::write_outcome::incomplete;

pub fn validate(name: &str, response: &Value) -> Result<()> {
    for warning in response.get("warnings").and_then(Value::as_array).into_iter().flatten() {
        let state = match warning.get("code").and_then(Value::as_str) {
            Some("PARTIAL_WRITE") => "Partial",
            Some("OUTCOME_UNKNOWN") => "Unknown",
            _ => continue,
        };
        return Err(incomplete(name, state, operation_id(warning)));
    }
    if response.pointer("/error/code").and_then(Value::as_str) == Some("OUTCOME_UNKNOWN") {
        return Err(incomplete(name, "Unknown", response.get("error").and_then(operation_id)));
    }
    anyhow::ensure!(
        response.get("error").is_some_and(Value::is_null),
        "{name} returned an error; operation_id={:?}",
        response.get("error").and_then(operation_id)
    );
    if matches!(name, "calendar_create" | "calendar_delete") {
        let id = response.get("data").and_then(operation_id);
        match response.pointer("/data/status").and_then(Value::as_str) {
            Some("succeeded") => (),
            Some("partial") => return Err(incomplete(name, "Partial", id)),
            Some("unknown") => return Err(incomplete(name, "Unknown", id)),
            Some("failed") => anyhow::bail!("{name} failed; operation_id={id:?}"),
            _ => return Err(incomplete(name, "Unverified", id)),
        }
    }
    Ok(())
}

fn operation_id(value: &Value) -> Option<&str> {
    value.get("operation_id").and_then(Value::as_str)
}
