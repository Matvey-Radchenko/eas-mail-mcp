use clap::{Args, Subcommand};

use super::common::InputSource;
use super::input::{ensure_flag_mode, read_json, required};
use super::output::{self, OutputKind, OutputMode};
use crate::cli::CliExit;
use crate::{PeopleSearchInput, Result, Runtime};

#[derive(Debug, Subcommand)]
pub(in crate::cli) enum PeopleCommand {
    /// Search one directory for names and email addresses.
    Search(PeopleSearchArgs),
}

#[derive(Debug, Args)]
pub(in crate::cli) struct PeopleSearchArgs {
    /// Account ID; required when multiple accounts are enabled.
    #[arg(long)]
    account: Option<String>,
    /// Name or email prefix.
    #[arg(long)]
    query: Option<String>,
    /// Maximum results, default 20 and maximum 50.
    #[arg(long)]
    limit: Option<u32>,
    #[command(flatten)]
    source: InputSource,
}

pub(super) async fn run(
    runtime: &Runtime,
    command: PeopleCommand,
    mode: OutputMode,
) -> Result<CliExit> {
    let PeopleCommand::Search(arguments) = command;
    let has_flags =
        arguments.account.is_some() || arguments.query.is_some() || arguments.limit.is_some();
    ensure_flag_mode(arguments.source.input.as_ref(), has_flags)?;
    let input = arguments.source.input.map_or_else(
        || {
            Ok(PeopleSearchInput {
                account_id: arguments.account,
                query: required(arguments.query, "query")?,
                limit: arguments.limit,
            })
        },
        |path| read_json(&path),
    )?;
    output::emit(runtime.people_search(input).await, mode, OutputKind::People, true)
}
