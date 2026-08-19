use std::fs;
use std::path::Path;

use eas_mail_profile::{ProfileBundle, ProfileSpec};
use eas_mail_protocol::ProfileKey;

use super::*;
use crate::{AccountConfig, AppConfig, save_config};

mod edge_cases;

#[test]
fn import_validate_list_and_export_round_trip() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    paths.ensure()?;
    let source = directory.path().join("incoming.toml");
    fs::write(&source, include_str!("../../../../../profile.example.toml"))?;

    let imported = import(&paths, ProfileImportArgs { file: source, replace: false, yes: false })?;
    assert_eq!(imported.get("count").and_then(serde_json::Value::as_u64), Some(1));
    assert_eq!(
        list(&paths)?.get("profiles").and_then(serde_json::Value::as_array).map(Vec::len),
        Some(1)
    );
    assert_eq!(
        validate(&paths, ProfileValidateArgs { file: None })?
            .get("count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );

    let exported = directory.path().join("exported.toml");
    export(&paths, ProfileExportArgs { file: exported.clone(), id: None })?;
    let stored = eas_mail_profile::load(&paths.profiles)?;
    let copy = eas_mail_profile::load(&exported)?;
    assert_eq!(stored.manifest, copy.manifest);
    Ok(())
}

#[test]
fn conflicting_replace_is_atomic_and_validates_existing_accounts() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    paths.ensure()?;
    import_fixture(&paths, directory.path())?;

    let mut replacement = stored_manifest(&paths)?;
    let profile = replacement
        .profiles
        .first_mut()
        .ok_or_else(|| anyhow::anyhow!("profile fixture is empty"))?;
    profile.host = "other.example.invalid".into();
    let replacement_path = write_manifest(directory.path(), "replacement.toml", &replacement)?;
    assert!(
        import(
            &paths,
            ProfileImportArgs { file: replacement_path.clone(), replace: false, yes: false },
        )
        .is_err()
    );
    import(&paths, ProfileImportArgs { file: replacement_path, replace: true, yes: true })?;
    assert_eq!(first_profile(&paths)?.host, "other.example.invalid");

    save_account(&paths)?;
    let mut invalid = stored_manifest(&paths)?;
    let profile =
        invalid.profiles.first_mut().ok_or_else(|| anyhow::anyhow!("profile fixture is empty"))?;
    profile.email_domains = vec!["other.invalid".into()];
    let invalid_path = write_manifest(directory.path(), "invalid.toml", &invalid)?;
    assert!(
        import(&paths, ProfileImportArgs { file: invalid_path, replace: true, yes: true },)
            .is_err()
    );
    assert_eq!(first_profile(&paths)?.email_domains, vec!["example.invalid"]);
    Ok(())
}

#[test]
fn used_profile_cannot_change_device_length_or_be_removed() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    paths.ensure()?;
    import_fixture(&paths, directory.path())?;
    save_account(&paths)?;

    let mut changed = stored_manifest(&paths)?;
    let profile =
        changed.profiles.first_mut().ok_or_else(|| anyhow::anyhow!("profile fixture is empty"))?;
    profile.device_id_length = 32;
    let changed_path = write_manifest(directory.path(), "changed.toml", &changed)?;
    assert!(
        import(&paths, ProfileImportArgs { file: changed_path, replace: true, yes: true },)
            .is_err()
    );
    assert!(
        remove(&paths, ProfileRemoveArgs { id: ProfileKey::new("example")?, yes: true },).is_err()
    );
    assert!(!paths.journal.exists());
    Ok(())
}

#[test]
fn manual_add_and_unused_remove_leave_no_empty_store() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    paths.ensure()?;
    add(
        &paths,
        ProfileAddArgs {
            id: Some("manual".into()),
            display_name: Some("Manual profile".into()),
            host: Some("mail.example.invalid".into()),
            email_domains: vec!["example.invalid".into()],
            username_realm: None,
            device_id_length: Some(16),
            pem: None,
        },
    )?;
    remove(&paths, ProfileRemoveArgs { id: ProfileKey::new("manual")?, yes: true })?;
    assert!(!paths.profiles.exists());
    Ok(())
}

fn import_fixture(paths: &Paths, root: &Path) -> anyhow::Result<()> {
    let source = root.join("fixture.toml");
    fs::write(&source, include_str!("../../../../../profile.example.toml"))?;
    import(paths, ProfileImportArgs { file: source, replace: false, yes: false })?;
    Ok(())
}

fn stored_manifest(paths: &Paths) -> anyhow::Result<ProfileBundle> {
    Ok(eas_mail_profile::load(&paths.profiles)?.manifest)
}

fn first_profile(paths: &Paths) -> anyhow::Result<ProfileSpec> {
    stored_manifest(paths)?
        .profiles
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("profile fixture is empty"))
}

fn write_manifest(
    root: &Path,
    name: &str,
    manifest: &ProfileBundle,
) -> anyhow::Result<std::path::PathBuf> {
    let path = root.join(name);
    fs::write(&path, eas_mail_profile::serialize(manifest)?)?;
    Ok(path)
}

fn save_account(paths: &Paths) -> anyhow::Result<()> {
    let mut config = AppConfig::default();
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
    Ok(())
}

fn paths(root: &Path) -> Paths {
    Paths {
        support: root.join("support"),
        attachments: root.join("cache/attachments"),
        config: root.join("support/config.toml"),
        profiles: root.join("support/profiles.toml"),
        journal: root.join("support/operations.sqlite"),
    }
}
