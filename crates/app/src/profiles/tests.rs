use std::fs;

use super::*;
use crate::ErrorCode;

#[test]
fn profile_store_round_trips_and_builds_registry() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("support/profiles.toml");
    assert!(load_profile_bundle(&path)?.is_none());
    assert!(load_profile_registry(&path)?.is_none());

    let manifest = example_manifest()?;
    let saved = save_profile_bundle(&path, &manifest)?;
    let loaded = load_profile_bundle(&path)?
        .ok_or_else(|| anyhow::anyhow!("saved profile bundle is missing"))?;
    assert_eq!(loaded.manifest, manifest);
    assert_eq!(loaded.hash, saved.hash);

    let registry = require_profile_registry(&path)?;
    assert_eq!(registry.profiles().len(), 1);
    assert_eq!(example_registry()?.profiles().len(), 1);
    Ok(())
}

#[test]
fn profile_store_maps_absent_invalid_and_storage_failures() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let missing = directory.path().join("missing.toml");
    let absent = require_profile_registry(&missing)
        .err()
        .ok_or_else(|| anyhow::anyhow!("missing store unexpectedly loaded"))?;
    assert_eq!(absent.envelope.code, ErrorCode::ConfigInvalid);
    assert!(absent.envelope.remediation.is_some());

    fs::write(&missing, "not = [valid")?;
    let invalid = load_profile_bundle(&missing)
        .err()
        .ok_or_else(|| anyhow::anyhow!("invalid store unexpectedly loaded"))?;
    assert_eq!(invalid.envelope.code, ErrorCode::ConfigInvalid);

    let mut invalid_manifest = example_manifest()?;
    invalid_manifest.profiles.clear();
    let invalid_save =
        save_profile_bundle(&directory.path().join("invalid.toml"), &invalid_manifest)
            .err()
            .ok_or_else(|| anyhow::anyhow!("invalid manifest unexpectedly saved"))?;
    assert_eq!(invalid_save.envelope.code, ErrorCode::ConfigInvalid);

    let blocker = directory.path().join("blocker");
    fs::write(&blocker, "file")?;
    let storage = save_profile_bundle(&blocker.join("profiles.toml"), &example_manifest()?)
        .err()
        .ok_or_else(|| anyhow::anyhow!("unsafe path unexpectedly saved"))?;
    assert_eq!(storage.envelope.code, ErrorCode::StorageError);
    Ok(())
}

fn example_manifest() -> anyhow::Result<ProfileBundle> {
    Ok(eas_mail_profile::parse(include_str!("../../../../profile.example.toml"))?.manifest)
}
