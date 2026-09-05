mod auto_reply;
mod calendar;
mod calendar_args;
mod calendar_input;
mod calendar_recurrence;
mod common;
mod human;
mod human_slots;
mod input;
mod mail;
mod mail_args;
mod mail_input;
mod mail_mutation_args;
mod mail_mutations;
mod output;
mod people;
mod writes;

use clap::{Args, Subcommand};

pub(super) use calendar_args::CalendarCommand;
pub(super) use mail_args::MailCommand;
use output::{OutputKind, OutputMode};
pub(super) use people::PeopleCommand;

use crate::cli::CliExit;
use crate::{AccountSelection, Runtime};

#[derive(Debug, Subcommand)]
pub(super) enum FolderCommand {
    /// Refresh and list Exchange folders.
    List(FolderListArgs),
}

#[derive(Debug, Args)]
pub(super) struct FolderListArgs {
    /// Account ID; repeat to select multiple accounts.
    #[arg(long = "account")]
    accounts: Vec<String>,
    #[command(flatten)]
    source: common::InputSource,
}

pub(super) const fn output_mode(human: bool) -> OutputMode {
    if human { OutputMode::Human } else { OutputMode::Json }
}

pub(super) fn accounts(runtime: &Runtime, mode: OutputMode) -> crate::Result<CliExit> {
    output::emit(runtime.accounts_list(), mode, OutputKind::Accounts, true)
}

pub(super) async fn folders(
    runtime: &Runtime,
    command: FolderCommand,
    mode: OutputMode,
) -> crate::Result<CliExit> {
    match command {
        FolderCommand::List(arguments) => {
            let has_flags = !arguments.accounts.is_empty();
            input::ensure_flag_mode(arguments.source.input.as_ref(), has_flags)?;
            let selection = arguments.source.input.map_or_else(
                || Ok(AccountSelection { account_ids: input::selected(arguments.accounts) }),
                |path| input::read_json(&path),
            )?;
            output::emit(runtime.folders_list(selection).await, mode, OutputKind::Folders, true)
        }
    }
}

pub(super) async fn mail(
    runtime: &Runtime,
    command: MailCommand,
    mode: OutputMode,
) -> crate::Result<CliExit> {
    mail::run(runtime, command, mode).await
}

pub(super) async fn people(
    runtime: &Runtime,
    command: PeopleCommand,
    mode: OutputMode,
) -> crate::Result<CliExit> {
    people::run(runtime, command, mode).await
}

pub(super) async fn calendar(
    runtime: &Runtime,
    command: CalendarCommand,
    mode: OutputMode,
) -> crate::Result<CliExit> {
    calendar::run(runtime, command, mode).await
}
