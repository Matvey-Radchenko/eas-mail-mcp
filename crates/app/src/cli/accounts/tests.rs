#![expect(
    clippy::indexing_slicing,
    reason = "fixed test fixtures use direct indexing for readable assertions"
)]

use eas_mail_protocol::ProfileKey;

use super::super::{AddAccountArgs, SetupArgs};
use super::*;

#[test]
fn explicit_setup_arguments_do_not_prompt() -> anyhow::Result<()> {
    let profiles = crate::profiles::example_registry()?;
    let profile = ProfileKey::new("example")?;
    let request = interactive_request(
        SetupArgs {
            profile_file: None,
            account_id: Some("work".into()),
            profile: Some(profile.clone()),
            email: Some("user@example.invalid".into()),
            username: Some("example_user".into()),
            password_stdin: true,
            enable_writes: false,
            skip_clients: true,
        },
        &profiles,
    )?;
    assert_eq!(request.account_id, "work");
    assert_eq!(request.profile, profile);
    assert!(request.password_stdin);
    assert!(!request.write_enabled);
    assert!(
        interactive_request(
            SetupArgs {
                profile_file: None,
                account_id: Some(" ".into()),
                profile: Some(ProfileKey::new("example")?),
                email: Some("user@example.invalid".into()),
                username: Some("example_user".into()),
                password_stdin: true,
                enable_writes: false,
                skip_clients: true,
            },
            &profiles
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn add_arguments_map_profiles_and_flags() -> anyhow::Result<()> {
    let profile = ProfileKey::new("example")?;
    let request = request(AddAccountArgs {
        account_id: "sample".into(),
        profile: profile.clone(),
        email: "user@example.invalid".into(),
        username: "example_user".into(),
        password_stdin: false,
        enable_writes: true,
    });
    assert_eq!(request.profile, profile);
    assert!(request.write_enabled);
    Ok(())
}

#[test]
fn passwords_and_required_values_fail_closed() -> anyhow::Result<()> {
    assert_eq!(required(Some(" value ".into()), "ignored")?, " value ");
    assert!(required(Some(String::new()), "ignored").is_err());
    for invalid in ["", "line\nfeed", "carriage\rreturn", "nul\0byte"] {
        assert!(validate_password(invalid).is_err());
    }
    validate_password("fixture-value")?;
    Ok(())
}

#[test]
fn list_and_write_toggle_persist_only_non_secret_config() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    let mut config = crate::AppConfig::default();
    config.accounts.insert(
        "work".into(),
        AccountConfig {
            profile: ProfileKey::new("example")?,
            email: "user@example.invalid".into(),
            username: "example_user".into(),
            enabled: true,
            write_enabled: false,
        },
    );
    save_config(&paths.config, &config)?;
    let listed = list(&paths)?;
    assert_eq!(listed["accounts"][0]["account_id"], "work");
    assert!(listed.to_string().find("example_user").is_none());

    let enabled = set_writes(&paths, "work", true)?;
    assert_eq!(enabled["write_enabled"], true);
    assert!(load_config(&paths.config)?.accounts["work"].write_enabled);
    assert_eq!(
        set_writes(&paths, "missing", true).map_err(|error| error.envelope.code),
        Err(ErrorCode::NotFound)
    );
    Ok(())
}

fn paths(root: &std::path::Path) -> Paths {
    Paths {
        support: root.join("support"),
        attachments: root.join("attachments"),
        config: root.join("support/config.toml"),
        profiles: root.join("support/profiles.toml"),
        journal: root.join("support/operations.sqlite"),
    }
}
