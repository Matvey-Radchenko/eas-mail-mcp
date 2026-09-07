mod actions;

use eas_mail_protocol::ProfileRegistry;

use self::actions::{SetupActions, SystemActions};
use super::terminal::Terminal;
use super::{ProfileAddArgs, SetupArgs, accounts, profiles};
use crate::profiles::require_profile_registry;
use crate::{AppError, ErrorCode, Paths, Result, load_config};

pub(super) async fn run(
    paths: &Paths,
    arguments: SetupArgs,
    terminal: &mut dyn Terminal,
) -> Result<serde_json::Value> {
    run_with_actions(paths, arguments, terminal, &SystemActions).await
}

async fn run_with_actions(
    paths: &Paths,
    mut arguments: SetupArgs,
    terminal: &mut dyn Terminal,
    actions: &dyn SetupActions,
) -> Result<serde_json::Value> {
    let explicit_account = arguments.has_account_arguments();
    let skip_clients = arguments.skip_clients;
    let profile_file = arguments.profile_file.take();
    let (mut registry, profile_result) =
        profiles::ensure_for_setup_with_terminal(paths, profile_file.as_deref(), terminal)?;
    let initially_empty = load_config(&paths.config)?.accounts.is_empty();
    let mut account_results = Vec::new();
    let mut client_results = Vec::new();

    if explicit_account || initially_empty {
        account_results.push(add_account(paths, arguments, &registry, terminal, actions).await?);
        while terminal.is_interactive() && terminal.confirm("Add another account", false)? {
            account_results
                .push(add_account(paths, blank_setup_args(), &registry, terminal, actions).await?);
        }
        if !skip_clients {
            client_results = actions.configure_clients(paths, terminal)?;
        }
    } else if terminal.is_interactive() {
        management_loop(
            paths,
            &mut registry,
            terminal,
            skip_clients,
            &mut account_results,
            &mut client_results,
            actions,
        )
        .await?;
    }

    let diagnostics = actions.doctor(paths, &registry).await?;
    if terminal.is_interactive() {
        print_setup_summary(terminal, load_config(&paths.config)?.accounts.len(), &client_results)?;
    }
    Ok(serde_json::json!({
        "profiles": profile_result,
        "accounts": account_results,
        "clients": client_results,
        "doctor": diagnostics,
    }))
}

fn print_setup_summary(
    terminal: &mut dyn Terminal,
    account_count: usize,
    client_results: &[serde_json::Value],
) -> Result<()> {
    terminal.message("Setup completed successfully")?;
    terminal.message(&format!("Accounts: {account_count} configured"))?;
    if client_results.is_empty() {
        return Ok(());
    }

    terminal.message("AI clients:")?;
    for result in client_results {
        let name = result
            .get("display_name")
            .or_else(|| result.get("client"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Unknown client");
        let status =
            if result.get("configured").and_then(serde_json::Value::as_bool).unwrap_or(false) {
                "configured, restart required"
            } else {
                "skipped"
            };
        terminal.message(&format!("  {name}: {status}"))?;
    }
    Ok(())
}

async fn add_account(
    paths: &Paths,
    arguments: SetupArgs,
    registry: &ProfileRegistry,
    terminal: &mut dyn Terminal,
    actions: &dyn SetupActions,
) -> Result<serde_json::Value> {
    let mut supplied = Some(arguments);
    loop {
        let request = accounts::collect_request(
            paths,
            supplied.take().unwrap_or_else(blank_setup_args),
            registry,
            terminal,
        )?;
        let prompt_for_writes = terminal.is_interactive() && !request.write_enabled;
        match actions.add_account(paths, request, registry, terminal).await {
            Ok(mut result) => {
                let account_id = result
                    .get("account_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        AppError::new(ErrorCode::ProtocolError, "account setup result is invalid")
                    })?;
                let writes_supported = result
                    .get("writes_supported")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                terminal.message(&format!("Account {account_id} connected successfully"))?;
                if prompt_for_writes && writes_supported {
                    if terminal.confirm(
                        "Enable supported mail and calendar writes, including sending and automatic replies",
                        false,
                    )? {
                        actions.set_verified_writes(paths, &account_id)?;
                        result
                            .as_object_mut()
                            .ok_or_else(|| {
                                AppError::new(
                                    ErrorCode::ProtocolError,
                                    "account setup result is invalid",
                                )
                            })?
                            .insert("write_enabled".into(), serde_json::Value::Bool(true));
                    }
                } else if prompt_for_writes {
                    terminal
                        .message("Exchange did not advertise the complete write command set")?;
                }
                return Ok(result);
            }
            Err(error) if terminal.is_interactive() => {
                terminal.message(&format!(
                    "Account check failed: {} ({})",
                    error.envelope.message,
                    error.envelope.code.as_str()
                ))?;
                if !terminal.confirm("Enter the account details again", true)? {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
    }
}

async fn management_loop(
    paths: &Paths,
    registry: &mut ProfileRegistry,
    terminal: &mut dyn Terminal,
    skip_clients: bool,
    account_results: &mut Vec<serde_json::Value>,
    client_results: &mut Vec<serde_json::Value>,
    actions: &dyn SetupActions,
) -> Result<()> {
    loop {
        let selection = terminal.select(
            "EAS Mail MCP setup",
            &[
                "Add another account".into(),
                "Repair an account email, login, or profile".into(),
                "Update an account password".into(),
                "Change account write access".into(),
                "Manage endpoint profiles".into(),
                "Configure AI clients".into(),
                "Run diagnostics".into(),
                "Finish".into(),
            ],
            0,
        )?;
        match selection {
            0 => account_results
                .push(add_account(paths, blank_setup_args(), registry, terminal, actions).await?),
            1 => {
                let account_id = select_account(paths, terminal)?;
                account_results
                    .push(actions.repair_account(paths, &account_id, registry, terminal).await?);
            }
            2 => {
                let account_id = select_account(paths, terminal)?;
                account_results
                    .push(actions.update_password(paths, &account_id, registry, terminal).await?);
            }
            3 => {
                let account_id = select_account(paths, terminal)?;
                let config = load_config(&paths.config)?;
                let enabled =
                    config.accounts.get(&account_id).is_some_and(|account| account.write_enabled);
                let requested = terminal.confirm(
                    "Enable supported mail and calendar writes, including sending and automatic replies",
                    enabled,
                )?;
                account_results.push(
                    actions.set_writes_checked(paths, &account_id, requested, registry).await?,
                );
            }
            4 => {
                manage_profiles(paths, terminal)?;
                *registry = require_profile_registry(&paths.profiles)?;
            }
            5 if !skip_clients => {
                client_results.extend(actions.configure_clients(paths, terminal)?);
            }
            5 => terminal.message("Client configuration was disabled for this setup run")?,
            6 => {
                let result = actions.doctor(paths, registry).await?;
                terminal.message(&format_doctor(&result))?;
            }
            _ => return Ok(()),
        }
    }
}

fn manage_profiles(paths: &Paths, terminal: &mut dyn Terminal) -> Result<()> {
    match terminal.select(
        "Endpoint profiles",
        &[
            "Import a profile file".into(),
            "Create a profile manually".into(),
            "List configured profiles".into(),
            "Back".into(),
        ],
        0,
    )? {
        0 => {
            profiles::import_from_prompt(paths, terminal)?;
        }
        1 => {
            profiles::add_with_terminal(paths, blank_profile_args(), terminal)?;
        }
        2 => terminal.message(&profiles::list(paths)?.to_string())?,
        _ => {}
    }
    Ok(())
}

fn select_account(paths: &Paths, terminal: &mut dyn Terminal) -> Result<String> {
    let config = load_config(&paths.config)?;
    let accounts = config.accounts.into_iter().collect::<Vec<_>>();
    let options = accounts
        .iter()
        .map(|(account_id, account)| format!("{} ({account_id})", account.email))
        .collect::<Vec<_>>();
    let selected = terminal.select("Select an account", &options, 0)?;
    accounts
        .get(selected)
        .map(|(account_id, _)| account_id.clone())
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "account is not configured"))
}

fn blank_setup_args() -> SetupArgs {
    SetupArgs {
        profile_file: None,
        account_id: None,
        profile: None,
        email: None,
        username: None,
        password_stdin: false,
        enable_writes: false,
        skip_clients: true,
    }
}

fn blank_profile_args() -> ProfileAddArgs {
    ProfileAddArgs {
        id: None,
        display_name: None,
        host: None,
        email_domains: Vec::new(),
        identity_mode: None,
        username_realm: None,
        username_hint: None,
        device_id_length: None,
        pem: None,
    }
}

fn format_doctor(value: &serde_json::Value) -> String {
    let accounts = value.get("accounts").and_then(serde_json::Value::as_array).map_or(0, Vec::len);
    format!("Diagnostics completed for {accounts} account(s)")
}

#[cfg(test)]
mod tests;
