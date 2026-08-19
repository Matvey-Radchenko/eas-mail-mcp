use std::error::Error;
use std::fs;

use rcgen::generate_simple_self_signed;
use sha2::{Digest as _, Sha256};

use super::{ProfileError, TrustSpec, load, parse, serialize};

#[test]
fn system_profile_loads_and_round_trips() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("profile.toml");
    fs::write(&path, system_manifest("mail.example.invalid", "example.invalid", None))?;
    let bundle = load(&path)?;
    assert_eq!(bundle.manifest.bundle_version, "fixture-1");
    assert_eq!(bundle.manifest.profiles.len(), 1);
    let profile = bundle
        .manifest
        .profiles
        .first()
        .ok_or_else(|| std::io::Error::other("verified profile is missing"))?;
    assert!(matches!(profile.trust, TrustSpec::System));
    assert_eq!(bundle.hash.len(), 64);
    assert!(bundle.source.is_some());
    let serialized = serialize(&bundle.manifest)?;
    assert_eq!(parse(&serialized)?.manifest, bundle.manifest);
    Ok(())
}

#[test]
fn exclusive_pem_is_verified_and_loaded() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let certified = generate_simple_self_signed(vec!["mail.example.invalid".into()])?;
    let pem = certified.cert.pem();
    let fingerprint = Sha256::digest(certified.cert.der().as_ref())
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    let path = directory.path().join("profile.toml");
    fs::write(&path, pem_manifest(&pem, &fingerprint))?;
    let bundle = load(&path)?;
    let profile = bundle
        .manifest
        .profiles
        .first()
        .ok_or_else(|| std::io::Error::other("verified profile is missing"))?;
    assert!(matches!(&profile.trust, TrustSpec::ExclusivePem { pem: value, .. } if value == &pem));
    Ok(())
}

#[test]
fn malformed_and_invalid_profile_fields_are_rejected() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("profile.toml");
    fs::write(&path, "not = [valid")?;
    assert!(matches!(load(&path), Err(ProfileError::Toml)));

    let invalid = [
        system_manifest("localhost", "example.invalid", None),
        system_manifest("mail.example.invalid:443", "example.invalid", None),
        system_manifest("mail.example.invalid", "not a domain", None),
        system_manifest("mail.example.invalid", "example.invalid", Some("BAD\\REALM")),
        system_manifest("mail.example.invalid", "example.invalid", Some("")),
        system_manifest("mail.example.invalid", "example.invalid", None)
            .replace("id = \"example\"", "id = \"Bad Key\""),
        system_manifest("mail.example.invalid", "example.invalid", None)
            .replace("device_id_length = 16", "device_id_length = 15"),
        system_manifest("mail.example.invalid", "example.invalid", None)
            .replace("schema_version = 1", "schema_version = 2"),
        system_manifest("mail.example.invalid", "example.invalid", None)
            + "\n[[profiles]]\nid = \"example\"\ndisplay_name = \"Duplicate\"\nhost = \"other.example.invalid\"\nemail_domains = [\"example.invalid\"]\ndevice_id_length = 16\n[profiles.trust]\nmode = \"system\"\n",
    ];
    for document in invalid {
        fs::write(&path, document)?;
        assert!(load(&path).is_err());
    }
    Ok(())
}

#[test]
fn trust_path_mode_and_fingerprint_fail_closed() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("profile.toml");
    for document in [
        pem_manifest("not a certificate", &"00".repeat(32)),
        pem_manifest("not a certificate", "not-a-fingerprint"),
        system_manifest("mail.example.invalid", "example.invalid", None)
            .replace("mode = \"system\"", "mode = \"unsupported\""),
    ] {
        fs::write(&path, document)?;
        assert!(load(&path).is_err());
    }

    let certified = generate_simple_self_signed(vec!["mail.example.invalid".into()])?;
    fs::write(&path, pem_manifest(&certified.cert.pem(), &"00".repeat(32)))?;
    assert!(matches!(load(&path), Err(ProfileError::Trust(_))));
    Ok(())
}

#[cfg(unix)]
#[test]
fn trust_path_rejects_symlinks() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir()?;
    let target = directory.path().join("actual.toml");
    fs::write(&target, system_manifest("mail.example.invalid", "example.invalid", None))?;
    let path = directory.path().join("profile.toml");
    symlink(&target, &path)?;
    assert!(matches!(load(&path), Err(ProfileError::Read)));
    Ok(())
}

fn system_manifest(host: &str, domain: &str, realm: Option<&str>) -> String {
    let realm = realm.map_or_else(String::new, |value| format!("username_realm = {value:?}\n"));
    format!(
        "schema_version = 1\nbundle_version = \"fixture-1\"\n\n[[profiles]]\nid = \"example\"\ndisplay_name = \"Example EAS\"\nhost = {host:?}\nemail_domains = [{domain:?}]\n{realm}device_id_length = 16\n\n[profiles.trust]\nmode = \"system\"\n"
    )
}

fn pem_manifest(pem: &str, fingerprint: &str) -> String {
    format!(
        "schema_version = 1\nbundle_version = \"fixture-1\"\n\n[[profiles]]\nid = \"example\"\ndisplay_name = \"Example EAS\"\nhost = \"mail.example.invalid\"\nemail_domains = [\"example.invalid\"]\ndevice_id_length = 16\n\n[profiles.trust]\nmode = \"exclusive_pem\"\npem = '''{pem}'''\nsha256 = {fingerprint:?}\n"
    )
}
