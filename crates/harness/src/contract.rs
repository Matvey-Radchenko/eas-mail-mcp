//! Semantic MCP contract helpers shared by release verification and black-box tests.

use std::collections::BTreeSet;

use anyhow::{Context as _, Result};
use serde_json::{Value, json};

/// Explicitly reviewed 1.0 baseline; package patch versions are intentionally excluded.
pub const BASELINE: &str = include_str!("../../../contracts/v1.0.json");

/// Returns the single source of truth for expected release tool names.
pub fn expected_tool_names() -> Result<BTreeSet<String>> {
    let value: Value = serde_json::from_str(BASELINE)?;
    let tools = value.get("mcp").and_then(Value::as_object).context("missing MCP contract")?;
    anyhow::ensure!(!tools.is_empty(), "MCP baseline is empty; run cargo xtask contract accept");
    Ok(tools.keys().cloned().collect())
}

/// Captures behavior-bearing tool fields while excluding documentation-only changes.
pub fn snapshot(tools: &[rmcp::model::Tool]) -> Result<Value> {
    let mut result = serde_json::Map::new();
    for tool in tools {
        let mut annotations = serde_json::to_value(&tool.annotations)?;
        if let Some(fields) = annotations.as_object_mut() {
            fields.remove("title");
        }
        result.insert(
            tool.name.to_string(),
            json!({
                "input":normalize_schema(Value::Object(tool.input_schema.as_ref().clone())),
                "output":normalize_schema(serde_json::to_value(&tool.output_schema)?),
                "annotations":annotations,
            }),
        );
    }
    Ok(Value::Object(result))
}

fn normalize_schema(value: Value) -> Value {
    let Value::Object(fields) = value else {
        return value;
    };
    let mut result = serde_json::Map::new();
    for (key, value) in fields {
        if matches!(key.as_str(), "description" | "title" | "examples" | "$schema") {
            continue;
        }
        let value = match key.as_str() {
            "properties" | "$defs" | "definitions" | "patternProperties" | "dependentSchemas" => {
                match value {
                    Value::Object(entries) => Value::Object(
                        entries
                            .into_iter()
                            .map(|(name, schema)| (name, normalize_schema(schema)))
                            .collect(),
                    ),
                    other => other,
                }
            }
            "anyOf" | "oneOf" | "allOf" => normalize_array(value, true, true),
            "prefixItems" => normalize_array(value, true, false),
            "required" | "type" | "enum" => normalize_array(value, false, true),
            "items"
            | "additionalProperties"
            | "contains"
            | "not"
            | "if"
            | "then"
            | "else"
            | "propertyNames"
            | "unevaluatedProperties" => normalize_schema(value),
            _ => value,
        };
        result.insert(key, value);
    }
    Value::Object(result)
}

fn normalize_array(value: Value, schemas: bool, sort: bool) -> Value {
    let Value::Array(mut items) = value else {
        return value;
    };
    if schemas {
        items = items.into_iter().map(normalize_schema).collect();
    }
    if sort {
        items.sort_by_key(Value::to_string);
    }
    Value::Array(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_ignores_docs_but_preserves_fields_literals_and_tuple_order() {
        let schema = json!({"title":"Docs", "properties":{"title":{"type":"string","description":"Text"}},
            "required":["z","a"],"enum":[{"description":"literal"}],
            "prefixItems":[{"type":"string"},{"type":"integer"}]});
        let normalized = normalize_schema(schema);
        assert!(normalized.get("title").is_none());
        assert!(normalized.pointer("/properties/title").is_some());
        assert_eq!(normalized.pointer("/enum/0/description"), Some(&json!("literal")));
        assert_eq!(normalized.pointer("/prefixItems/0/type"), Some(&json!("string")));
        assert_eq!(normalized.get("required"), Some(&json!(["a", "z"])));
    }
}
