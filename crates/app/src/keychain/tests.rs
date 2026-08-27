use eas_mail_protocol::protocol::evaluate_policy;

use super::*;

#[cfg(any(target_os = "macos", windows))]
#[test]
fn native_credential_store_is_persistent() {
    assert!(matches!(
        keyring::default::default_credential_builder().persistence(),
        keyring::credential::CredentialPersistence::UntilDelete
    ));
}

#[test]
fn version_one_bundle_and_device_id_validate() -> anyhow::Result<()> {
    let mut bundle = SecretBundle::new();
    assert_eq!(bundle.hmac_key.len(), 32);
    let device_id = SecretBundle::device_id(16)?;
    assert_eq!(device_id.len(), 16);
    assert!(device_id.bytes().all(|byte| byte.is_ascii_hexdigit()));
    bundle.accounts.insert("work".into(), secret(device_id));
    bundle.validate()?;

    bundle.version = 2;
    assert!(bundle.validate().is_err());
    bundle.version = 1;
    bundle.hmac_key.clear();
    assert!(bundle.validate().is_err());
    Ok(())
}

#[test]
fn zero_policy_key_cannot_have_policy_limits() {
    let mut bundle = SecretBundle::new();
    let mut value = secret("0011223344556677".into());
    value.policy_key = 0;
    assert_invalid(&mut bundle, value.clone());
    value.policy = None;
    value.policy_key = 9;
    bundle.accounts.insert("work".into(), value.clone());
    assert!(bundle.validate().is_ok());
    value.device_id = "bad-device-id".into();
    value.policy_key = 0;
    assert_invalid(&mut bundle, value);
}

#[test]
fn legacy_bundle_without_policy_deserializes_and_requires_refresh() -> anyhow::Result<()> {
    let input = r#"{
        "version": 1,
        "hmac_key": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        "accounts": {"work": {"password": "fixture", "device_id": "0011223344556677", "policy_key": 7}}
    }"#;
    let bundle: SecretBundle = serde_json::from_str(input)?;
    assert!(bundle.accounts.get("work").is_some_and(|value| value.policy.is_none()));
    bundle.validate()?;
    Ok(())
}

#[test]
fn stored_policy_round_trips_enforceable_limits() {
    let decision = evaluate_policy(&BTreeMap::new());
    let stored = StoredPolicy::from(&decision);
    assert_eq!(PolicyDecision::from(&stored), decision);
}

#[test]
fn memory_store_supports_update_delete_and_recreate() -> anyhow::Result<()> {
    let store = MemorySecretStore::default();
    store.update(&mut |bundle| {
        bundle.accounts.insert("work".into(), secret("0011223344556677".into()));
        Ok(())
    })?;
    assert!(store.load()?.accounts.contains_key("work"));
    store.delete()?;
    assert!(store.load()?.accounts.is_empty());
    Ok(())
}

#[test]
fn failed_memory_update_does_not_commit_partial_state() -> anyhow::Result<()> {
    let store = MemorySecretStore::default();
    let result = store.update(&mut |bundle| {
        bundle.accounts.insert("work".into(), secret("0011223344556677".into()));
        Err(AppError::new(ErrorCode::StorageError, "fixture failure"))
    });
    assert!(result.is_err());
    assert!(store.load()?.accounts.is_empty());
    Ok(())
}

#[test]
fn credential_capacity_error_is_actionable_and_redacted() -> anyhow::Result<()> {
    let error = keychain_error(keyring::Error::TooLong("private fixture attribute".into(), 2560));
    assert_eq!(error.envelope.code, ErrorCode::StorageError);
    assert!(!error.envelope.retryable);
    assert!(error.envelope.message.contains("per-entry size limit"));
    assert!(error.envelope.remediation.as_deref().is_some_and(|value| {
        value.contains("all accounts in one credential entry")
            && value.contains("Remove unused accounts")
    }));
    assert!(!serde_json::to_string(&error.envelope)?.contains("private fixture attribute"));
    Ok(())
}

#[test]
fn other_credential_errors_keep_the_unlock_remediation() {
    let error = keychain_error(keyring::Error::NoEntry);
    assert_eq!(error.envelope.code, ErrorCode::AuthRequired);
    assert_eq!(
        error.envelope.remediation.as_deref(),
        Some("Unlock the user credential store and retry")
    );
}

fn secret(device_id: String) -> AccountSecret {
    let decision = evaluate_policy(&BTreeMap::new());
    AccountSecret {
        password: "fixture-value".into(),
        device_id,
        policy_key: 7,
        policy: Some(StoredPolicy::from(&decision)),
    }
}

fn assert_invalid(bundle: &mut SecretBundle, value: AccountSecret) {
    bundle.accounts.clear();
    bundle.accounts.insert("work".into(), value);
    assert!(bundle.validate().is_err());
}
