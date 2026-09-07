mod cli;
mod schema;

use anyhow::{Context as _, Result};
use serde_json::Value;

pub(super) fn check(previous: &Value, current: &Value) -> Result<()> {
    let old_tools =
        previous.get("mcp").and_then(Value::as_object).context("old MCP contract missing")?;
    let new_tools =
        current.get("mcp").and_then(Value::as_object).context("new MCP contract missing")?;
    for (name, old) in old_tools {
        let new = new_tools.get(name).with_context(|| format!("removed tool {name}"))?;
        for (field, output) in [("input", false), ("output", true)] {
            let before = old.get(field).context("schema missing")?;
            let after = new.get(field).context("schema missing")?;
            let result =
                if output { schema::subset(after, before) } else { schema::subset(before, after) };
            result.with_context(|| format!("incompatible {field} schema for {name}"))?;
        }
        anyhow::ensure!(
            old.get("annotations") == new.get("annotations"),
            "changed behavior annotations for {name}"
        );
    }
    cli::check(
        previous.get("cli").context("old CLI missing")?,
        current.get("cli").context("new CLI missing")?,
        "CLI",
    )
}

#[cfg(test)]
mod tests;
