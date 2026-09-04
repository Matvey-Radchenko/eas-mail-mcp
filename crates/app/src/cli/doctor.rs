use std::path::PathBuf;
use std::sync::Arc;

use clap::Args;
use eas_mail_protocol::ProfileRegistry;
use futures::future::join_all;

use crate::backend::{AccountBackend as _, EasMailbox};
use crate::{AppError, ErrorCode, KeychainStore, Paths, Result, Runtime, SecretStore, load_config};

mod report;

#[derive(Debug, Args)]
pub(super) struct DoctorArgs {
    /// Exit with code 1 when configuration or an enabled account is unhealthy.
    #[arg(long)]
    check: bool,
    /// Save a support report without account identifiers, paths, or server details.
    #[arg(long, value_name = "PATH")]
    report: Option<PathBuf>,
}

pub(super) async fn execute(paths: &Paths, arguments: DoctorArgs) -> Result<super::CliExit> {
    let result = async {
        let profiles = crate::load_profile_registry(&paths.profiles)?;
        run(paths, profiles.as_ref()).await
    }
    .await;
    let safe_report = match &result {
        Ok(value) => report::SupportReport::from_diagnostics(value),
        Err(error) => report::SupportReport::failure(error.envelope.code),
    };
    if let Some(path) = arguments.report {
        safe_report.write(&path)?;
    }
    super::emit(&result?)?;
    Ok(if arguments.check && !safe_report.healthy {
        super::CliExit::Unhealthy
    } else {
        super::CliExit::Success
    })
}

pub(super) async fn run(
    paths: &Paths,
    registry: Option<&ProfileRegistry>,
) -> Result<serde_json::Value> {
    let config = load_config(&paths.config)?;
    let Some(registry) = registry else {
        return Ok(serde_json::json!({
            "config": "ok",
            "profile_store": "missing",
            "accounts_configured": config.accounts.len(),
            "remediation": "Run eas-mail-mcp setup or profile import",
        }));
    };
    config.validate()?;
    let store: Arc<dyn SecretStore> = Arc::new(KeychainStore::new(paths.journal.clone()));
    let bundle = store.load()?;
    let checks = config.accounts.into_iter().map(|(account_id, account)| {
        let store = Arc::clone(&store);
        let secret = bundle.accounts.get(&account_id).cloned();
        let paths = paths.clone();
        async move {
            if !account.enabled {
                return serde_json::json!({"account_id": account_id, "status": "disabled"});
            }
            if let Err(error) = account.validate(registry) {
                return redacted_failure(account_id, error);
            }
            let Some(secret) = secret else {
                return serde_json::json!({
                    "account_id": account_id,
                    "status": "auth_required",
                    "code": "AUTH_REQUIRED",
                });
            };
            match EasMailbox::production_with_secret(
                account_id.clone(),
                account,
                store,
                secret,
                registry,
            ) {
                Ok(mailbox) => match mailbox.capabilities().await {
                    Ok(capabilities) => match mailbox.folders().await {
                        Ok(folders) => serde_json::json!({
                            "account_id": account_id,
                            "status": "ok",
                            "folders": folders.len(),
                            "server_write_permission": null,
                            "capabilities": {
                                "calendar_availability": if capabilities.calendar_availability {
                                    "available"
                                } else {
                                    "unsupported"
                                },
                                "mail_writes": capabilities.mail_writes,
                                "personal_calendar_writes": capabilities.personal_calendar_writes,
                                "meeting_lifecycle": capabilities.meeting_lifecycle,
                                "auto_reply": capabilities.auto_reply,
                                "mail_move": capabilities.mail_move,
                                "mail_properties": capabilities.mail_properties,
                            },
                        }),
                        Err(error) => redacted_account_failure(&paths, account_id, error),
                    },
                    Err(error) => redacted_account_failure(&paths, account_id, error),
                },
                Err(error) => redacted_account_failure(&paths, account_id, error),
            }
        }
    });
    let accounts = join_all(checks).await;
    Ok(serde_json::json!({
        "config": "ok",
        "keychain": "ok",
        "tls": "mandatory",
        "redirects": "disabled",
        "profile_store": {
            "version": registry.bundle_version(),
            "sha256": registry.bundle_hash(),
            "profiles": registry.profiles().len(),
        },
        "accounts": accounts,
    }))
}

fn redacted_account_failure(
    paths: &Paths,
    account_id: String,
    error: AppError,
) -> serde_json::Value {
    if error.envelope.code == ErrorCode::RemoteWipe
        && let Err(cleanup) = Runtime::purge_persisted_account(paths, &account_id)
    {
        return redacted_failure(account_id, cleanup);
    }
    redacted_failure(account_id, error)
}

fn redacted_failure(account_id: String, error: AppError) -> serde_json::Value {
    let code = serde_json::to_value(error.envelope.code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| ErrorCode::ProtocolError.as_str().into());
    serde_json::json!({
        "account_id": account_id,
        "status": "failed",
        "code": code,
        "retryable": error.envelope.retryable,
        "remediation": error.envelope.remediation,
    })
}

#[cfg(test)]
mod tests;
