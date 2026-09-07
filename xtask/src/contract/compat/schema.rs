use anyhow::{Context as _, Result};
use serde_json::{Map, Value};

#[path = "schema_object.rs"]
mod object;

/// Proves containment for the schema constructs this application emits; uncertain changes fail closed.
pub(super) fn subset(narrow: &Value, wide: &Value) -> Result<()> {
    if narrow == wide {
        return Ok(());
    }
    Context { narrow, wide }.compare(narrow, wide, "$", 0)
}

struct Context<'a> {
    narrow: &'a Value,
    wide: &'a Value,
}

impl Context<'_> {
    fn compare(&self, narrow: &Value, wide: &Value, path: &str, depth: usize) -> Result<()> {
        anyhow::ensure!(depth < 64, "{path}: recursive schema requires manual review");
        let narrow = resolve(narrow, self.narrow)?;
        let wide = resolve(wide, self.wide)?;
        if narrow == &Value::Bool(false) || wide == &Value::Bool(true) {
            return Ok(());
        }
        anyhow::ensure!(wide != &Value::Bool(false), "{path}: values are no longer accepted");
        let empty = Map::new();
        let a = narrow.as_object().unwrap_or(&empty);
        let b = wide.as_object().unwrap_or(&empty);
        if let Some(branches) = union(a)? {
            for branch in branches {
                self.compare(&branch, wide, path, depth + 1)?;
            }
            return Ok(());
        }
        if let Some(branches) = union(b)? {
            anyhow::ensure!(
                branches.iter().any(|branch| self.compare(narrow, branch, path, depth + 1).is_ok()),
                "{path}: union does not cover previously accepted values"
            );
            return Ok(());
        }
        check_types(a, b, path)?;
        check_enums(a, b, path)?;
        for (kind, min, max) in [
            ("string", "minLength", "maxLength"),
            ("array", "minItems", "maxItems"),
            ("object", "minProperties", "maxProperties"),
        ] {
            if permits(a, kind) {
                bounds(a, b, min, max, path)?;
            }
        }
        if permits(a, "number") || permits(a, "integer") {
            bounds(a, b, "minimum", "maximum", path)?;
            // Changed exclusive bounds are conservatively rejected unless the receiver removes them.
            for key in ["exclusiveMinimum", "exclusiveMaximum", "multipleOf"] {
                constraint(a, b, key, path)?;
            }
        }
        if permits(a, "string") {
            for key in ["pattern", "format"] {
                constraint(a, b, key, path)?;
            }
        }
        if permits(a, "array") {
            let any = Value::Bool(true);
            self.compare(
                a.get("items").unwrap_or(&any),
                b.get("items").unwrap_or(&any),
                &format!("{path}/items"),
                depth + 1,
            )?;
            anyhow::ensure!(
                b.get("uniqueItems") != Some(&Value::Bool(true))
                    || a.get("uniqueItems") == Some(&Value::Bool(true)),
                "{path}: uniqueness tightened"
            );
        }
        if permits(a, "object") {
            object::check(self, a, b, path, depth + 1)?;
        }
        unsupported(a, b, path)
    }
}

fn resolve<'a>(value: &'a Value, root: &'a Value) -> Result<&'a Value> {
    if let Some(reference) = value.get("$ref").and_then(Value::as_str) {
        anyhow::ensure!(
            value
                .as_object()
                .is_some_and(|v| v.keys().all(|k| matches!(k.as_str(), "$ref" | "default"))),
            "reference siblings require manual review"
        );
        let pointer = reference
            .strip_prefix('#')
            .context("external schema reference requires manual review")?;
        root.pointer(pointer).context("unresolved schema reference")
    } else {
        Ok(value)
    }
}

fn union(fields: &Map<String, Value>) -> Result<Option<Vec<Value>>> {
    let Some((key, branches)) = ["anyOf", "oneOf"]
        .into_iter()
        .find_map(|k| fields.get(k).and_then(Value::as_array).map(|v| (k, v)))
    else {
        return Ok(None);
    };
    anyhow::ensure!(
        fields.keys().all(|k| k == key || matches!(k.as_str(), "$defs" | "default")),
        "union siblings require manual review"
    );
    if key == "oneOf" {
        // Equality is handled by subset(); changed exclusive unions need an explicit review.
        anyhow::bail!("changed oneOf schema requires manual review");
    }
    Ok(Some(branches.clone()))
}

fn types(fields: &Map<String, Value>) -> Vec<&str> {
    match fields.get("type") {
        Some(Value::String(value)) => vec![value],
        Some(Value::Array(values)) => values.iter().filter_map(Value::as_str).collect(),
        _ => vec!["null", "boolean", "object", "array", "number", "integer", "string"],
    }
}

fn permits(fields: &Map<String, Value>, kind: &str) -> bool {
    types(fields).contains(&kind)
}

fn check_types(a: &Map<String, Value>, b: &Map<String, Value>, path: &str) -> Result<()> {
    let accepted = types(b);
    anyhow::ensure!(
        types(a)
            .iter()
            .all(|kind| accepted.contains(kind)
                || (*kind == "integer" && accepted.contains(&"number"))),
        "{path}: type set tightened"
    );
    Ok(())
}

fn values(fields: &Map<String, Value>) -> Option<Vec<&Value>> {
    fields
        .get("const")
        .map(|v| vec![v])
        .or_else(|| fields.get("enum").and_then(Value::as_array).map(|v| v.iter().collect()))
}

fn check_enums(a: &Map<String, Value>, b: &Map<String, Value>, path: &str) -> Result<()> {
    if let Some(accepted) = values(b) {
        let possible =
            values(a).with_context(|| format!("{path}: unrestricted values became an enum"))?;
        anyhow::ensure!(
            possible.iter().all(|v| accepted.contains(v)),
            "{path}: enum values removed"
        );
    }
    Ok(())
}

fn bounds(
    a: &Map<String, Value>,
    b: &Map<String, Value>,
    min: &str,
    max: &str,
    path: &str,
) -> Result<()> {
    for (key, acceptable) in [(min, std::cmp::Ordering::Greater), (max, std::cmp::Ordering::Less)] {
        if let Some(limit) = b.get(key) {
            let actual = a.get(key).with_context(|| format!("{path}: {key} tightened"))?;
            let ordering = numeric_order(actual, limit)
                .with_context(|| format!("{path}: {key} comparison requires review"))?;
            anyhow::ensure!(
                ordering == acceptable || ordering == std::cmp::Ordering::Equal,
                "{path}: {key} tightened"
            );
        }
    }
    Ok(())
}

fn numeric_order(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    let integer = |v: &Value| v.as_i64().map(i128::from).or_else(|| v.as_u64().map(i128::from));
    if let (Some(a), Some(b)) = (integer(a), integer(b)) {
        return Some(a.cmp(&b));
    }
    // Avoid silently rounding large exact integer bounds through f64.
    if [a, b].iter().any(|value| integer(value).is_some_and(|v| v.abs() > (1_i128 << 53))) {
        return None;
    }
    a.as_f64()?.partial_cmp(&b.as_f64()?)
}

fn constraint(a: &Map<String, Value>, b: &Map<String, Value>, key: &str, path: &str) -> Result<()> {
    anyhow::ensure!(
        !b.contains_key(key) || a.get(key) == b.get(key),
        "{path}: changed {key} requires review"
    );
    Ok(())
}

fn unsupported(a: &Map<String, Value>, b: &Map<String, Value>, path: &str) -> Result<()> {
    let known = [
        "type",
        "enum",
        "const",
        "$defs",
        "definitions",
        "properties",
        "required",
        "additionalProperties",
        "items",
        "uniqueItems",
        "minLength",
        "maxLength",
        "minItems",
        "maxItems",
        "minProperties",
        "maxProperties",
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "multipleOf",
        "pattern",
        "format",
    ];
    for key in a.keys().chain(b.keys()) {
        if !known.contains(&key.as_str()) {
            anyhow::ensure!(
                a.get(key) == b.get(key) && !contains_ref(a.get(key)),
                "{path}: changed or unresolved {key} requires review"
            );
        }
    }
    Ok(())
}

fn contains_ref(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Object(v)) => {
            ["$ref", "$dynamicRef", "$recursiveRef"].iter().any(|key| v.contains_key(*key))
                || v.values().any(|v| contains_ref(Some(v)))
        }
        Some(Value::Array(v)) => v.iter().any(|v| contains_ref(Some(v))),
        _ => false,
    }
}
