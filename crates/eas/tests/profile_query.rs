#![expect(
    clippy::indexing_slicing,
    reason = "fixed test fixtures use direct indexing for readable assertions"
)]

use base64::Engine as _;

use eas_mail_protocol::{Command, EasError, ProfileKey, ProfileRegistry, build_binary_query};

#[test]
fn runtime_profile_has_fixed_endpoint_and_identity_rules() -> eas_mail_protocol::Result<()> {
    let registry = ProfileRegistry::from_toml(include_str!("../../../profile.example.toml"))?;
    let key = ProfileKey::new("example")?;
    let profile = registry.require(&key)?;
    assert_eq!(profile.key(), "example");
    assert_eq!(profile.display_name(), "Example EAS");
    assert_eq!(profile.endpoint(), "https://mail.example.invalid/Microsoft-Server-ActiveSync");
    assert!(!profile.has_extra_trust_anchor());
    assert_eq!(profile.device_id_length(), 16);
    assert!(profile.validate_identity("user@EXAMPLE.INVALID", "example_user").is_ok());
    assert!(profile.validate_identity("user@other.invalid", "example_user").is_err());
    assert!(profile.validate_identity("user@example.invalid", "").is_err());
    assert!(profile.validate_identity("user@example.invalid", "bad\nname").is_err());
    assert!(profile.validate_device_id("0011223344556677").is_ok());
    assert!(profile.validate_device_id("001122").is_err());
    assert!(registry.require(&ProfileKey::new("missing")?).is_err());
    assert_eq!(registry.bundle_version(), "example-1");
    assert_eq!(registry.bundle_hash().len(), 64);
    assert!(!registry.is_empty());
    Ok(())
}

#[test]
fn profile_keys_reject_runtime_endpoint_material() {
    for invalid in ["", "Uppercase", "has space", "host.example.invalid", "-leading"] {
        assert!(ProfileKey::new(invalid).is_err());
    }
}

#[test]
fn compact_query_encodes_every_command_and_policy_form() -> eas_mail_protocol::Result<()> {
    let commands = [
        (Command::Sync, 0x00),
        (Command::SendMail, 0x01),
        (Command::SmartForward, 0x02),
        (Command::SmartReply, 0x03),
        (Command::FolderSync, 0x09),
        (Command::Search, 0x10),
        (Command::ItemOperations, 0x13),
        (Command::Provision, 0x14),
    ];
    for (command, expected) in commands {
        let query = build_binary_query(command, "001122AABBCC", 0x1234_5678, false)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(query)
            .map_err(|_| EasError::Protocol("query was not base64".into()))?;
        assert_eq!(bytes[1], expected);
        assert!(bytes.windows(4).any(|window| window == 0x1234_5678_u32.to_le_bytes()));
        assert!(bytes.ends_with(b"EasMailMCP"));
    }

    let omitted = base64::engine::general_purpose::STANDARD
        .decode(build_binary_query(Command::Provision, "ABC123", 99, true)?)
        .map_err(|_| EasError::Protocol("query was not base64".into()))?;
    assert_eq!(omitted[11], 0);
    assert!(build_binary_query(Command::Sync, &"A".repeat(32), 0, false).is_ok());
    for invalid in ["", "has-dash", "with space", "123456789012345678901234567890123"] {
        assert!(build_binary_query(Command::Sync, invalid, 0, false).is_err());
    }
    Ok(())
}
