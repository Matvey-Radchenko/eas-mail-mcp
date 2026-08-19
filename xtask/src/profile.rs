use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use eas_mail_profile::{TrustSpec, VerifiedBundle, load};

pub(crate) fn verify(root: &Path, path: &Path) -> Result<VerifiedBundle> {
    let source = resolve(root, path);
    let bundle = load(&source).with_context(|| format!("cannot verify {}", path.display()))?;
    let profiles = bundle
        .manifest
        .profiles
        .iter()
        .map(|profile| {
            serde_json::json!({
                "id": profile.id,
                "trust": match profile.trust {
                    TrustSpec::System => "system",
                    TrustSpec::ExclusivePem { .. } => "exclusive_pem",
                },
                "device_id_length": profile.device_id_length,
            })
        })
        .collect::<Vec<_>>();
    let report = serde_json::json!({
        "schema_version": bundle.manifest.schema_version,
        "bundle_version": bundle.manifest.bundle_version,
        "bundle_hash": bundle.hash,
        "profiles": profiles,
    });
    writeln!(io::stdout().lock(), "{}", serde_json::to_string_pretty(&report)?)?;
    Ok(bundle)
}

fn resolve(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() { path.to_owned() } else { root.join(path) }
}
