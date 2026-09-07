use super::*;
use crate::MemorySecretStore;
use eas_mail_protocol::ProfileKey;

#[tokio::test]
async fn missing_credentials_retain_account_and_return_auth_required() -> anyhow::Result<()> {
    let backend = configured_backend(
        "work".into(),
        account()?,
        Arc::new(MemorySecretStore::default()),
        None,
        &crate::profiles::example_registry()?,
    );
    let metadata = backend.account();
    assert_eq!(metadata.account_id, "work");
    assert!(metadata.enabled);
    assert!(metadata.write_enabled);
    let error = backend.configuration_error().ok_or_else(|| anyhow::anyhow!("missing failure"))?;
    assert_eq!(error.code, ErrorCode::AuthRequired);
    assert_eq!(error.account_id.as_deref(), Some("work"));
    assert_eq!(backend.capabilities().await.map_err(code), Err(ErrorCode::AuthRequired));
    assert_eq!(backend.folders().await.map_err(code), Err(ErrorCode::AuthRequired));
    Ok(())
}

#[test]
fn disabled_account_retains_metadata_without_requiring_its_profile_or_secret() -> anyhow::Result<()>
{
    let mut config = account()?;
    config.enabled = false;
    config.profile = ProfileKey::new("removed")?;
    let backend = configured_backend(
        "disabled".into(),
        config,
        Arc::new(MemorySecretStore::default()),
        None,
        &crate::profiles::example_registry()?,
    );
    assert!(!backend.account().enabled);
    assert!(backend.account().email_domains.is_empty());
    assert_eq!(backend.account().account_id, "disabled");
    assert_eq!(
        backend.configuration_error().map(|error| error.code),
        Some(ErrorCode::ConfigInvalid)
    );
    Ok(())
}

#[test]
fn invalid_profile_and_missing_secret_do_not_prevent_another_backend_starting() -> anyhow::Result<()>
{
    let profiles = crate::profiles::example_registry()?;
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::default());
    let mut invalid = account()?;
    invalid.profile = ProfileKey::new("missing")?;
    let bad = configured_backend("bad".into(), invalid, secrets.clone(), Some(secret()), &profiles);
    let missing =
        configured_backend("missing".into(), account()?, secrets.clone(), None, &profiles);
    let healthy =
        configured_backend("healthy".into(), account()?, secrets, Some(secret()), &profiles);
    assert_eq!(bad.configuration_error().map(|error| error.code), Some(ErrorCode::ConfigInvalid));
    assert_eq!(
        missing.configuration_error().map(|error| error.code),
        Some(ErrorCode::AuthRequired)
    );
    assert!(healthy.configuration_error().is_none());
    assert_eq!(healthy.account().account_id, "healthy");
    assert!(healthy.account().enabled);
    Ok(())
}

fn account() -> anyhow::Result<AccountConfig> {
    Ok(AccountConfig {
        profile: ProfileKey::new("example")?,
        email: "user@example.invalid".into(),
        username: "example_user".into(),
        enabled: true,
        write_enabled: true,
    })
}

fn secret() -> AccountSecret {
    AccountSecret {
        password: "fixture-value".into(),
        device_id: "0011223344556677".into(),
        policy_key: 0,
        policy: None,
    }
}

fn code(error: AppError) -> ErrorCode {
    error.envelope.code
}
