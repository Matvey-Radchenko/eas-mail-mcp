use serde_json::Value;

use crate::{AppError, ErrorCode, Result};

pub(super) fn render(value: &Value, recurring: bool) -> Result<String> {
    let suggestions = value.get("suggestions").and_then(Value::as_array).ok_or_else(invalid)?;
    let mut lines = vec![format!(
        "Precision: {} minutes; buffer: {} minutes; results truncated: {}",
        field(value, "precision_minutes"),
        field(value, "buffer_minutes"),
        field(value, "results_truncated")
    )];
    if suggestions.is_empty() {
        lines.push("No results".into());
    }
    for suggestion in suggestions {
        if recurring {
            lines.push(format!(
                "Weekly {}: required participants available on {} occurrences",
                field(suggestion, "local_start_time"),
                field(suggestion, "required_available_occurrences")
            ));
            for occurrence in
                suggestion.get("occurrences").and_then(Value::as_array).ok_or_else(invalid)?
            {
                lines.push(slot(occurrence));
            }
        } else {
            lines.push(slot(suggestion));
        }
    }
    Ok(lines.join("\n"))
}

fn slot(value: &Value) -> String {
    format!(
        "{} - {}\n  conflicts: {}\n  accepted tentative: {}",
        field(value, "starts_at"),
        field(value, "ends_at"),
        field(value, "conflicts"),
        field(value, "tentative_participants")
    )
}

fn field(value: &Value, name: &str) -> String {
    value.get(name).map_or_else(|| "null".into(), Value::to_string)
}

fn invalid() -> AppError {
    AppError::new(ErrorCode::ProtocolError, "cannot render scheduling output")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_slots_keep_conflicts_and_truncation_explicit() -> anyhow::Result<()> {
        let result = render(
            &serde_json::json!({"precision_minutes":30,"buffer_minutes":15,
            "results_truncated":true,"suggestions":[{"starts_at":"s","ends_at":"e",
            "conflicts":[{"input":"person","role":"optional","reasons":["unknown"]}],
            "tentative_participants":[]}]}),
            false,
        )?;
        assert!(result.contains("results truncated: true"));
        assert!(result.contains("optional") && result.contains("unknown"));
        Ok(())
    }
}
