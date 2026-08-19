mod account_secrets;
mod accounts;
mod clients;
mod doctor;
mod profiles;

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand, ValueEnum};
use eas_mail_protocol::ProfileKey;

use crate::profiles::require_profile_registry;
use crate::{AppError, ErrorCode, Paths, Result, Runtime, load_config, load_profile_registry};

/// Direct stdio MCP and local administration CLI.
#[derive(Debug, Parser)]
#[command(name = "eas-mail-mcp", about, disable_version_flag = true)]
struct Cli {
    /// Print application version information.
    #[arg(long)]
    version: bool,
    /// Include local profile-store metadata with --version.
    #[arg(long, requires = "version")]
    verbose: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the MCP server over stdin/stdout.
    Serve,
    /// Interactively add one managed Exchange account.
    Setup(SetupArgs),
    /// Manage account configuration and Keychain credentials.
    Account {
        #[command(subcommand)]
        command: AccountCommand,
    },
    /// Run redacted configuration and live EAS diagnostics.
    Doctor,
    /// Register or remove the MCP from an AI client.
    Client {
        #[command(subcommand)]
        command: ClientCommand,
    },
    /// Manage local EAS endpoint profiles.
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Print the direct native binary path for manual MCP configuration.
    NativePath,
}

#[derive(Debug, Subcommand)]
enum ProfileCommand {
    /// Validate and import one or more profiles.
    Import(ProfileImportArgs),
    /// Interactively create one local profile.
    Add(ProfileAddArgs),
    /// Validate the local store or another profile file.
    Validate(ProfileValidateArgs),
    /// List configured profiles without certificate contents.
    List,
    /// Export all profiles or one selected profile.
    Export(ProfileExportArgs),
    /// Remove one unused profile.
    Remove(ProfileRemoveArgs),
}

#[derive(Debug, Args)]
struct ProfileImportArgs {
    /// Portable profile TOML file.
    file: PathBuf,
    /// Replace conflicting profile identifiers.
    #[arg(long)]
    replace: bool,
    /// Confirm replacement without an interactive prompt.
    #[arg(long, requires = "replace")]
    yes: bool,
}

#[derive(Debug, Args)]
struct ProfileAddArgs {
    /// Stable lowercase profile identifier.
    #[arg(long)]
    id: Option<String>,
    /// Human-readable profile name.
    #[arg(long)]
    display_name: Option<String>,
    /// Exchange DNS host without scheme, port, or path.
    #[arg(long)]
    host: Option<String>,
    /// Allowed mailbox domain; repeat for multiple domains.
    #[arg(long = "email-domain")]
    email_domains: Vec<String>,
    /// Required AD username realm.
    #[arg(long)]
    username_realm: Option<String>,
    /// Exact EAS Device ID length.
    #[arg(long)]
    device_id_length: Option<u8>,
    /// Exclusive PEM certificate file; omit to use the system trust store.
    #[arg(long)]
    pem: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ProfileValidateArgs {
    /// Profile file; defaults to the local profile store.
    file: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ProfileExportArgs {
    /// Destination TOML file.
    file: PathBuf,
    /// Export only one profile identifier.
    #[arg(long)]
    id: Option<ProfileKey>,
}

#[derive(Debug, Args)]
struct ProfileRemoveArgs {
    /// Profile identifier.
    id: ProfileKey,
    /// Confirm removal without an interactive prompt.
    #[arg(long)]
    yes: bool,
}

#[derive(Debug, Args)]
struct SetupArgs {
    /// Portable profile TOML to import when no local profiles exist.
    #[arg(long)]
    profile_file: Option<PathBuf>,
    /// Stable local account identifier.
    #[arg(long)]
    account_id: Option<String>,
    /// Managed Exchange profile.
    #[arg(long)]
    profile: Option<ProfileKey>,
    /// Mailbox address.
    #[arg(long)]
    email: Option<String>,
    /// Exchange or AD username.
    #[arg(long)]
    username: Option<String>,
    /// Read the password from stdin instead of a terminal prompt.
    #[arg(long)]
    password_stdin: bool,
    /// Enable write tools for this account; tool calls execute immediately.
    #[arg(long)]
    enable_writes: bool,
    /// Do not offer MCP client configuration.
    #[arg(long)]
    skip_clients: bool,
}

impl SetupArgs {
    fn has_account_arguments(&self) -> bool {
        self.account_id.is_some()
            || self.profile.is_some()
            || self.email.is_some()
            || self.username.is_some()
            || self.password_stdin
            || self.enable_writes
    }
}

#[derive(Debug, Subcommand)]
enum AccountCommand {
    /// List configured accounts without credentials.
    List,
    /// Add and live-verify an account.
    Add(AddAccountArgs),
    /// Replace and live-verify an account password.
    UpdatePassword(PasswordArgs),
    /// Enable or disable write tools for one account.
    SetWrites(ToggleArgs),
    /// Remove account configuration and credentials.
    Remove(AccountIdArgs),
}

#[derive(Debug, Args)]
struct AddAccountArgs {
    /// Stable local account identifier.
    account_id: String,
    /// Managed Exchange profile.
    #[arg(long)]
    profile: ProfileKey,
    /// Mailbox address.
    #[arg(long)]
    email: String,
    /// Exchange or AD username.
    #[arg(long)]
    username: String,
    /// Read the password from stdin instead of a terminal prompt.
    #[arg(long)]
    password_stdin: bool,
    /// Enable write tools for this account.
    #[arg(long)]
    enable_writes: bool,
}

#[derive(Debug, Args)]
struct PasswordArgs {
    /// Stable local account identifier.
    account_id: String,
    /// Read the password from stdin instead of a terminal prompt.
    #[arg(long)]
    password_stdin: bool,
}

#[derive(Debug, Args)]
struct ToggleArgs {
    /// Stable local account identifier.
    account_id: String,
    /// New state.
    #[arg(value_enum)]
    value: Toggle,
}

#[derive(Debug, Args)]
struct AccountIdArgs {
    /// Stable local account identifier.
    account_id: String,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Toggle {
    On,
    Off,
}

#[derive(Debug, Subcommand)]
enum ClientCommand {
    /// Add the MCP and remove obsolete generated write-approval rules.
    Configure(ClientArgs),
    /// Remove only entries managed by this application.
    Unconfigure(ClientArgs),
}

#[derive(Debug, Args)]
struct ClientArgs {
    /// Supported AI client.
    #[arg(value_enum)]
    client: clients::ClientKind,
    /// Override the client executable used for version detection and setup.
    #[arg(long)]
    executable: Option<String>,
}

/// Parses and runs one CLI command.
pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    if cli.version {
        if cli.command.is_some() {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                "--version cannot be combined with a command",
            ));
        }
        return emit_version(cli.verbose);
    }
    let command = cli.command.ok_or_else(|| {
        AppError::new(ErrorCode::ValidationFailed, "a command or --version is required")
    })?;
    if matches!(&command, Command::NativePath) {
        return emit_native_path();
    }
    let paths = Paths::standard()?;
    paths.ensure()?;
    match command {
        Command::Serve => {
            let profiles = require_profile_registry(&paths.profiles)?;
            let config = load_config(&paths.config)?;
            let runtime = Arc::new(Runtime::production(config, &paths, &profiles)?);
            crate::mcp::serve_stdio(runtime).await.map_err(|_| {
                AppError::new(ErrorCode::ProtocolError, "MCP stdio transport stopped unexpectedly")
            })
        }
        Command::Setup(arguments) => emit(&setup(&paths, arguments).await?),
        Command::Account { command } => {
            let profiles = load_profile_registry(&paths.profiles)?;
            emit(&accounts::run(&paths, command, profiles.as_ref()).await?)
        }
        Command::Doctor => {
            let profiles = load_profile_registry(&paths.profiles)?;
            emit(&doctor::run(&paths, profiles.as_ref()).await?)
        }
        Command::Client { command } => emit(&clients::run(&paths, command)?),
        Command::Profile { command } => emit(&profiles::run(&paths, command)?),
        Command::NativePath => {
            Err(AppError::new(ErrorCode::ProtocolError, "native path command dispatch is invalid"))
        }
    }
}

async fn setup(paths: &Paths, arguments: SetupArgs) -> Result<serde_json::Value> {
    let explicit_account = arguments.has_account_arguments();
    let skip_clients = arguments.skip_clients;
    let (profiles, profile_result) =
        profiles::ensure_for_setup(paths, arguments.profile_file.as_deref())?;
    let config = load_config(&paths.config)?;
    let account_result = if explicit_account || config.accounts.is_empty() {
        let request = accounts::interactive_request(arguments, &profiles)?;
        accounts::add(paths, request, &profiles).await?
    } else {
        config.validate_profiles(&profiles)?;
        serde_json::json!({ "reused": config.accounts.len() })
    };
    let clients = if skip_clients { Vec::new() } else { clients::configure_detected(paths)? };
    let diagnostics = doctor::run(paths, Some(&profiles)).await?;
    Ok(serde_json::json!({
        "profiles": profile_result,
        "accounts": account_result,
        "clients": clients,
        "doctor": diagnostics,
    }))
}

fn emit_version(verbose: bool) -> Result<()> {
    let mut output = std::io::stdout().lock();
    if !verbose {
        return writeln!(output, "eas-mail-mcp {}", env!("CARGO_PKG_VERSION"))
            .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot write CLI output"));
    }
    let paths = Paths::standard()?;
    let registry = load_profile_registry(&paths.profiles)?;
    let document = serde_json::json!({
        "name": "EAS Mail MCP",
        "binary": "eas-mail-mcp",
        "version": env!("CARGO_PKG_VERSION"),
        "profile_store": {
            "configured": registry.is_some(),
            "version": registry.as_ref().map(|value| value.bundle_version()),
            "sha256": registry.as_ref().map(|value| value.bundle_hash()),
            "profiles": registry.as_ref().map_or(0, |value| value.profiles().len()),
        },
    });
    let document = serde_json::to_string_pretty(&document)
        .map_err(|_| AppError::new(ErrorCode::ProtocolError, "cannot serialize CLI output"))?;
    writeln!(output, "{document}")
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot write CLI output"))
}

fn emit_native_path() -> Result<()> {
    let path = std::env::current_exe()
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot resolve native binary path"))?;
    writeln!(std::io::stdout().lock(), "{}", path.display())
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot write CLI output"))
}

fn emit(value: &serde_json::Value) -> Result<()> {
    let document = serde_json::to_string_pretty(value)
        .map_err(|_| AppError::new(ErrorCode::ProtocolError, "cannot serialize CLI output"))?;
    writeln!(std::io::stdout().lock(), "{document}")
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot write CLI output"))
}

fn prompt(label: &str) -> Result<String> {
    let mut stderr = std::io::stderr().lock();
    write!(stderr, "{label}: ")
        .and_then(|()| stderr.flush())
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot write terminal prompt"))?;
    let mut value = String::new();
    std::io::stdin()
        .read_line(&mut value)
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot read terminal input"))?;
    Ok(value.trim().to_owned())
}

fn confirm(label: &str) -> Result<bool> {
    let answer = prompt(&format!("{label} [y/N]"))?;
    Ok(matches!(answer.to_ascii_lowercase().as_str(), "y" | "yes"))
}
