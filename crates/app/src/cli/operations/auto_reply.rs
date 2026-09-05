use std::path::{Path, PathBuf};

use clap::{Args, Subcommand, ValueEnum};

use super::common::{InputSource, WriteControl};
use super::input;
use super::output::{self, OutputKind, OutputMode};
use super::writes;
use crate::cli::CliExit;
use crate::runtime::write_preview::PreparedWrite;
use crate::{
    ApiResponse, AutoReplyExternalAudience, AutoReplyGetInput, AutoReplyOperationResult,
    AutoReplyOperationState, AutoReplySetInput, AutoReplyState, Result, Runtime,
};

#[derive(Debug, Subcommand)]
pub(in crate::cli) enum AutoReplyCommand {
    /// Read current automatic-reply settings.
    Get(GetArgs),
    /// Set or disable automatic replies and verify the effective settings.
    Set(SetArgs),
}

#[derive(Debug, Args)]
pub(in crate::cli) struct GetArgs {
    #[command(flatten)]
    source: InputSource,
    /// Explicit local account identifier.
    #[arg(long)]
    account: Option<String>,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct SetArgs {
    #[command(flatten)]
    source: InputSource,
    #[command(flatten)]
    control: WriteControl,
    /// Explicit local account with writes enabled.
    #[arg(long)]
    account: Option<String>,
    /// Disable replies, enable indefinitely, or use a scheduled interval.
    #[arg(long, value_enum)]
    state: Option<StateArg>,
    /// Scheduled RFC3339 start with an explicit timezone offset.
    #[arg(long)]
    starts_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Scheduled RFC3339 end with an explicit timezone offset.
    #[arg(long)]
    ends_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Internal plain-text reply.
    #[arg(long, conflicts_with = "internal_message_file")]
    internal_message: Option<String>,
    /// Read the internal plain-text reply from a UTF-8 file.
    #[arg(long, value_name = "FILE")]
    internal_message_file: Option<PathBuf>,
    /// External audience; omitted means none.
    #[arg(long, value_enum)]
    external_audience: Option<AudienceArg>,
    /// External plain-text reply shared by known and unknown senders.
    #[arg(long, conflicts_with = "external_message_file")]
    external_message: Option<String>,
    /// Read the external plain-text reply from a UTF-8 file.
    #[arg(long, value_name = "FILE")]
    external_message_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum StateArg {
    Disabled,
    Enabled,
    Scheduled,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AudienceArg {
    None,
    Known,
    All,
}

pub(super) async fn run(
    runtime: &Runtime,
    command: AutoReplyCommand,
    mode: OutputMode,
) -> Result<CliExit> {
    match command {
        AutoReplyCommand::Get(arguments) => {
            input::ensure_flag_mode(arguments.source.input.as_ref(), arguments.account.is_some())?;
            let request = if let Some(path) = arguments.source.input {
                input::read_json(&path)?
            } else {
                AutoReplyGetInput { account_id: input::required(arguments.account, "--account")? }
            };
            output::emit(
                runtime.mail_get_auto_reply(request).await,
                mode,
                OutputKind::AutoReply,
                true,
            )
        }
        AutoReplyCommand::Set(arguments) => {
            let (request, yes) = set_input(arguments)?;
            let response = match runtime.prepare_cli_auto_reply(&request).await? {
                PreparedWrite::Replay(response) => response,
                PreparedWrite::Ready(preview) => {
                    let Some(fingerprint) = writes::approve(&preview, yes)? else {
                        return Ok(CliExit::Declined);
                    };
                    runtime.commit_cli_auto_reply(request, &fingerprint).await
                }
            };
            set_output(response, mode)
        }
    }
}

fn set_input(arguments: SetArgs) -> Result<(AutoReplySetInput, bool)> {
    let has_flags = arguments.account.is_some()
        || arguments.state.is_some()
        || arguments.starts_at.is_some()
        || arguments.ends_at.is_some()
        || arguments.internal_message.is_some()
        || arguments.internal_message_file.is_some()
        || arguments.external_audience.is_some()
        || arguments.external_message.is_some()
        || arguments.external_message_file.is_some();
    input::ensure_flag_mode(arguments.source.input.as_ref(), has_flags)?;
    if let Some(path) = arguments.source.input {
        return Ok((input::read_write_json(&path, &arguments.control)?, arguments.control.yes));
    }
    let state = match arguments.state.ok_or_else(|| input::invalid("--state is required"))? {
        StateArg::Disabled => AutoReplyState::Disabled,
        StateArg::Enabled => AutoReplyState::Enabled,
        StateArg::Scheduled => AutoReplyState::Scheduled,
    };
    let external_audience = match arguments.external_audience.unwrap_or(AudienceArg::None) {
        AudienceArg::None => AutoReplyExternalAudience::None,
        AudienceArg::Known => AutoReplyExternalAudience::Known,
        AudienceArg::All => AutoReplyExternalAudience::All,
    };
    Ok((
        AutoReplySetInput {
            account_id: input::required(arguments.account, "--account")?,
            state,
            starts_at: arguments.starts_at,
            ends_at: arguments.ends_at,
            internal_message: message(
                arguments.internal_message,
                arguments.internal_message_file.as_deref(),
            )?,
            external_audience,
            external_message: message(
                arguments.external_message,
                arguments.external_message_file.as_deref(),
            )?,
            idempotency_key: input::idempotency_key(&arguments.control),
        },
        arguments.control.yes,
    ))
}

fn message(inline: Option<String>, file: Option<&Path>) -> Result<Option<String>> {
    if let Some(file) = file {
        std::fs::read_to_string(file)
            .map(Some)
            .map_err(|_| input::invalid("cannot read automatic-reply message file"))
    } else {
        Ok(inline)
    }
}

fn set_output(
    response: ApiResponse<AutoReplyOperationResult>,
    mode: OutputMode,
) -> Result<CliExit> {
    let succeeded = response
        .data
        .as_ref()
        .is_some_and(|value| value.status == AutoReplyOperationState::Succeeded);
    output::emit(response, mode, OutputKind::Write, succeeded)
}
