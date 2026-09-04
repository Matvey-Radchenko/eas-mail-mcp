use anyhow::{Context as _, Result};
use serde_json::Value;

pub(super) fn check(old: &Value, new: &Value, path: &str) -> Result<()> {
    for key in [
        "subcommand_required",
        "arg_required_else_help",
        "args_conflict_with_subcommands",
        "subcommand_negates_requirements",
    ] {
        anyhow::ensure!(old.get(key) == new.get(key), "{path}: changed {key}");
    }
    contains_values(old.get("aliases"), new.get("aliases"), path)?;
    let old_args =
        old.get("arguments").and_then(Value::as_object).context("CLI arguments missing")?;
    let new_args =
        new.get("arguments").and_then(Value::as_object).context("CLI arguments missing")?;
    for (name, argument) in old_args {
        let current =
            new_args.get(name).with_context(|| format!("{path}: removed argument {name}"))?;
        for key in [
            "long",
            "short",
            "position",
            "action",
            "defaults",
            "global",
            "hidden",
            "allow_hyphen_values",
            "num_values",
            "conflicts",
        ] {
            anyhow::ensure!(argument.get(key) == current.get(key), "{path}/{name}: changed {key}");
        }
        anyhow::ensure!(
            argument.get("required") == Some(&Value::Bool(true))
                || current.get("required") != Some(&Value::Bool(true)),
            "{path}/{name}: argument became required"
        );
        contains_values(argument.get("aliases"), current.get("aliases"), path)?;
        choices(argument.get("possible_values"), current.get("possible_values"), path)?;
    }
    for (name, current) in new_args {
        anyhow::ensure!(
            old_args.contains_key(name) || current.get("required") != Some(&Value::Bool(true)),
            "{path}: new required argument {name}"
        );
    }
    let old_commands =
        old.get("subcommands").and_then(Value::as_object).context("CLI commands missing")?;
    let new_commands =
        new.get("subcommands").and_then(Value::as_object).context("CLI commands missing")?;
    for (name, command) in old_commands {
        check(
            command,
            new_commands.get(name).with_context(|| format!("{path}: removed command {name}"))?,
            &format!("{path}/{name}"),
        )?;
    }
    Ok(())
}

fn contains_values(old: Option<&Value>, new: Option<&Value>, path: &str) -> Result<()> {
    let before = old.and_then(Value::as_array).context("CLI value list missing")?;
    let after = new.and_then(Value::as_array).context("CLI value list missing")?;
    anyhow::ensure!(
        before.iter().all(|value| after.contains(value)),
        "{path}: accepted value removed"
    );
    Ok(())
}

fn choices(old: Option<&Value>, new: Option<&Value>, path: &str) -> Result<()> {
    let before = old.and_then(Value::as_array).context("CLI choices missing")?;
    let after = new.and_then(Value::as_array).context("CLI choices missing")?;
    anyhow::ensure!(
        after.is_empty()
            || (!before.is_empty() && before.iter().all(|value| after.contains(value))),
        "{path}: accepted CLI choices narrowed"
    );
    Ok(())
}
