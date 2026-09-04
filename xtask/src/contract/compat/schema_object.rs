use anyhow::Result;
use serde_json::{Map, Value};

use super::Context;

pub(super) fn check(
    context: &Context<'_>,
    a: &Map<String, Value>,
    b: &Map<String, Value>,
    path: &str,
    depth: usize,
) -> Result<()> {
    let required_a = required(a);
    anyhow::ensure!(
        required(b).iter().all(|name| required_a.contains(name)),
        "{path}: required fields tightened"
    );
    let empty = Map::new();
    let properties_a = a.get("properties").and_then(Value::as_object).unwrap_or(&empty);
    let properties_b = b.get("properties").and_then(Value::as_object).unwrap_or(&empty);
    let any = Value::Bool(true);
    let additional_a = a.get("additionalProperties").unwrap_or(&any);
    let additional_b = b.get("additionalProperties").unwrap_or(&any);
    for name in
        properties_a.keys().chain(properties_b.keys()).collect::<std::collections::BTreeSet<_>>()
    {
        let before = properties_a.get(name).unwrap_or(additional_a);
        let after = properties_b.get(name).unwrap_or(additional_b);
        context.compare(before, after, &format!("{path}/properties/{name}"), depth)?;
    }
    context.compare(additional_a, additional_b, &format!("{path}/additionalProperties"), depth)
}

fn required(fields: &Map<String, Value>) -> Vec<&str> {
    fields
        .get("required")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}
