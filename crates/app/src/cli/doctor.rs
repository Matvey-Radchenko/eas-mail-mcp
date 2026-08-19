use std::sync::Arc;

use eas_mail_protocol::ProfileRegistry;
use futures::future::join_all;

use crate::backend::{AccountBackend as _, EasMailbox};
use crate::{AppError, ErrorCode, KeychainStore, Paths, Result, Runtime, SecretStore, load_config};

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
    config.validate_profiles(registry)?;
    let store: Arc<dyn SecretStore> = Arc::new(KeychainStore::new(paths.journal.clone()));
    let bundle = store.load()?;
    let checks = config.accounts.into_iter().map(|(account_id, account)| {
        let store = Arc::clone(&store);
        let secret = bundle.accounts.get(&account_id).cloned();
        let paths = paths.clone();
        async move {
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
                Ok(mailbox) => match mailbox.folders().await {
                    Ok(folders) => serde_json::json!({
                        "account_id": account_id,
                        "status": "ok",
                        "folders": folders.len(),
                    }),
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
        .unwrap_or_else(|| code(ErrorCode::ProtocolError).into());
    serde_json::json!({
        "account_id": account_id,
        "status": "failed",
        "code": code,
        "retryable": error.envelope.retryable,
        "remediation": error.envelope.remediation,
    })
}

const fn code(value: ErrorCode) -> &'static str {
    match value {
        ErrorCode::AuthRequired => "AUTH_REQUIRED",
        ErrorCode::NetworkUnreachable => "NETWORK_UNREACHABLE",
        ErrorCode::ConfigInvalid => "CONFIG_INVALID",
        ErrorCode::PolicyBlocked => "POLICY_BLOCKED",
        ErrorCode::NotFound => "NOT_FOUND",
        ErrorCode::ReferenceExpired => "REFERENCE_EXPIRED",
        ErrorCode::ValidationFailed => "VALIDATION_FAILED",
        ErrorCode::ProtocolError => "PROTOCOL_ERROR",
        ErrorCode::SyncStale => "SYNC_STALE",
        ErrorCode::OutcomeUnknown => "OUTCOME_UNKNOWN",
        ErrorCode::RemoteWipe => "REMOTE_WIPE",
        ErrorCode::IdempotencyConflict => "IDEMPOTENCY_CONFLICT",
        ErrorCode::StorageError => "STORAGE_ERROR",
    }
}

#[cfg(test)]
mod tests;
