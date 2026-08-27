use std::fs;

use eas_mail_protocol::ProfileKey;

use super::*;

#[test]
fn config_round_trip_is_private_and_atomic() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nested/config.toml");
    let mut config = AppConfig::default();
    config.accounts.insert("example.work".into(), example()?);
    save_config(&path, &config)?;
    assert_eq!(load_config(&path)?, config);
    assert!(!directory.path().join("nested/.config-0.tmp").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
        assert_eq!(
            fs::metadata(path.parent().ok_or_else(path_error)?)?.permissions().mode() & 0o777,
            0o700
        );
    }
    Ok(())
}

#[test]
fn absent_and_invalid_configs_are_distinguished() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    assert_eq!(load_config(&path)?, AppConfig::default());

    fs::write(&path, "version = 2\n")?;
    assert_eq!(load_config(&path).map_err(code), Err(ErrorCode::ConfigInvalid));
    fs::write(&path, "version = 1\nunknown = true\n")?;
    assert_eq!(load_config(&path).map_err(code), Err(ErrorCode::ConfigInvalid));
    fs::write(&path, "not toml")?;
    assert_eq!(load_config(&path).map_err(code), Err(ErrorCode::ConfigInvalid));
    Ok(())
}

#[test]
fn account_identifiers_and_managed_domains_are_validated() -> anyhow::Result<()> {
    let profiles = crate::profiles::example_registry()?;
    let mut config = AppConfig::default();
    for id in ["", "has space", &"x".repeat(65)] {
        config.accounts.clear();
        config.accounts.insert(id.into(), example()?);
        assert_eq!(config.validate().map_err(code), Err(ErrorCode::ConfigInvalid));
    }
    config.accounts.clear();
    let mut account = example()?;
    account.email = "user@example.com".into();
    config.accounts.insert("valid-id_1".into(), account);
    assert_eq!(config.validate_profiles(&profiles).map_err(code), Err(ErrorCode::ConfigInvalid));
    Ok(())
}

#[test]
fn custom_paths_create_only_private_runtime_directories() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = Paths {
        support: directory.path().join("support"),
        attachments: directory.path().join("cache/attachments"),
        config: directory.path().join("support/config.toml"),
        profiles: directory.path().join("support/profiles.toml"),
        journal: directory.path().join("support/operations.sqlite"),
    };
    paths.ensure()?;
    assert!(paths.support.is_dir());
    assert!(paths.attachments.is_dir());
    let standard = Paths::standard()?;
    assert!(standard.config.ends_with("EAS Mail MCP/config.toml"));
    #[cfg(windows)]
    assert_eq!(
        standard.support,
        dirs::data_local_dir()
            .ok_or_else(|| anyhow::anyhow!("local application data directory is missing"))?
            .join("EAS Mail MCP")
    );
    Ok(())
}

fn example() -> anyhow::Result<AccountConfig> {
    Ok(AccountConfig {
        profile: ProfileKey::new("example")?,
        email: "user@example.invalid".into(),
        username: "example_user".into(),
        enabled: true,
        write_enabled: false,
    })
}

fn code(error: AppError) -> ErrorCode {
    error.envelope.code
}

#[cfg(unix)]
fn path_error() -> std::io::Error {
    std::io::Error::other("path has no parent")
}
