use clap::{Arg, Command, CommandFactory as _};
use serde_json::{Value, json};

use super::Cli;

/// Returns a content-free structural CLI contract for release compatibility checks.
#[doc(hidden)]
#[must_use]
pub fn snapshot() -> Value {
    let mut command = Cli::command();
    command.build();
    command_contract(&command)
}

fn command_contract(command: &Command) -> Value {
    let arguments = command
        .get_arguments()
        .map(|arg| (arg.get_id().as_str().to_owned(), argument_contract(command, arg)))
        .collect::<serde_json::Map<_, _>>();
    let subcommands = command
        .get_subcommands()
        .map(|child| (child.get_name().to_owned(), command_contract(child)))
        .collect::<serde_json::Map<_, _>>();
    json!({
        "arguments":arguments, "subcommands":subcommands,
        "aliases":command.get_all_aliases().collect::<Vec<_>>(),
        "subcommand_required":command.is_subcommand_required_set(),
        "arg_required_else_help":command.is_arg_required_else_help_set(),
        "args_conflict_with_subcommands":command.is_args_conflicts_with_subcommands_set(),
        "subcommand_negates_requirements":command.is_subcommand_negates_reqs_set(),
    })
}

fn argument_contract(command: &Command, argument: &Arg) -> Value {
    let cardinality = argument.get_num_args().map(|range| {
        json!({
            "min":range.min_values(), "max":range.max_values(),
        })
    });
    let mut conflicts = command
        .get_arg_conflicts_with(argument)
        .into_iter()
        .map(|arg| arg.get_id().as_str())
        .collect::<Vec<_>>();
    conflicts.sort_unstable();
    json!({
        "long":argument.get_long(), "short":argument.get_short(), "position":argument.get_index(),
        "aliases":argument.get_all_aliases().unwrap_or_default(),
        "action":format!("{:?}", argument.get_action()), "required":argument.is_required_set(),
        "global":argument.is_global_set(), "hidden":argument.is_hide_set(),
        "allow_hyphen_values":argument.is_allow_hyphen_values_set(), "num_values":cardinality,
        "defaults":argument.get_default_values().iter().map(|value|value.to_string_lossy()).collect::<Vec<_>>(),
        "possible_values":argument.get_possible_values().iter().map(|value|value.get_name()).collect::<Vec<_>>(),
        "conflicts":conflicts,
    })
}
