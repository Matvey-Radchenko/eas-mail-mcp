use std::collections::BTreeMap;

use base64::Engine as _;
use eas_mail_protocol::protocol::{
    build_initial_provision, build_mime_message, build_sync, evaluate_policy, parse_provision,
    parse_sync,
};
use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{ChangeData, ChangeKind, CollectionKind, Patch};

#[test]
fn initial_provision_matches_reference_client_bytes() -> eas_mail_protocol::Result<()> {
    let expected_base64 = if cfg!(windows) {
        "AwFqAAAORQASVkhXA0VBUyBNYWlsIE1DUAABWQNFQVMgTWFpbCBNQ1AAAVoDV2luZG93cwABWwNlbgABAQEADkZHSANNUy1FQVMtUHJvdmlzaW9uaW5nLVdCWE1MAAEBAQE="
    } else {
        "AwFqAAAORQASVkhXA0VBUyBNYWlsIE1DUAABWQNFQVMgTWFpbCBNQ1AAAVoDbWFjT1MAAVsDZW4AAQEBAA5GR0gDTVMtRUFTLVByb3Zpc2lvbmluZy1XQlhNTAABAQEB"
    };
    let expected = base64::engine::general_purpose::STANDARD
        .decode(expected_base64)
        .map_err(|error| eas_mail_protocol::EasError::Protocol(error.to_string()))?;
    assert_eq!(build_initial_provision()?, expected);
    Ok(())
}

#[test]
fn sync_change_preserves_empty_and_missing_fields() -> eas_mail_protocol::Result<()> {
    let mut root = Element::new("AirSync", "Sync");
    let mut collections = Element::new("AirSync", "Collections");
    let mut collection = Element::new("AirSync", "Collection");
    collection.push(Element::text("AirSync", "SyncKey", "12"));
    collection.push(Element::text("AirSync", "Status", "1"));
    let mut commands = Element::new("AirSync", "Commands");
    let mut change = Element::new("AirSync", "Change");
    change.push(Element::text("AirSync", "ServerId", "message-1"));
    let mut data = Element::new("AirSync", "ApplicationData");
    data.push(Element::text("Email", "Subject", ""));
    change.push(data);
    commands.push(change);
    collection.push(commands);
    collections.push(collection);
    root.push(collections);

    let page = parse_sync(&encode(&root)?, CollectionKind::Mail)?;
    let first = page
        .changes
        .first()
        .ok_or_else(|| eas_mail_protocol::EasError::Protocol("missing change".into()))?;
    assert_eq!(first.kind, ChangeKind::Change);
    let ChangeData::Mail(fields) = &first.data else {
        return Err(eas_mail_protocol::EasError::Protocol("wrong change data".into()));
    };
    assert_eq!(fields.subject, Patch::Value(String::new()));
    assert_eq!(fields.sender, Patch::Missing);
    Ok(())
}

#[test]
fn sync_requests_use_required_filters_and_metadata_preview() -> eas_mail_protocol::Result<()> {
    let mail = build_sync("inbox", "10", CollectionKind::Mail, 5, 500)?;
    let calendar = build_sync("calendar", "20", CollectionKind::Calendar, 6, 500)?;
    let mail_tree = eas_mail_protocol::wbxml::decode(&mail)?
        .ok_or_else(|| eas_mail_protocol::EasError::Protocol("empty mail request".into()))?;
    let calendar_tree = eas_mail_protocol::wbxml::decode(&calendar)?
        .ok_or_else(|| eas_mail_protocol::EasError::Protocol("empty calendar request".into()))?;
    assert_eq!(
        mail_tree.descendant("AirSync", "FilterType").map(Element::text_content),
        Some("5".into())
    );
    assert_eq!(
        mail_tree.descendant("AirSyncBase", "TruncationSize").map(Element::text_content),
        Some("500".into())
    );
    assert_eq!(
        calendar_tree.descendant("AirSync", "FilterType").map(Element::text_content),
        Some("6".into())
    );
    Ok(())
}

#[test]
fn mime_normalizes_lines_and_rejects_header_injection() -> eas_mail_protocol::Result<()> {
    let message = build_mime_message(
        "sender@example.invalid",
        &["recipient@example.invalid".into()],
        &[],
        &[],
        "Subject",
        "one\r\ntwo\rthree\nfour",
    )?;
    let text = String::from_utf8(message)
        .map_err(|_| eas_mail_protocol::EasError::Protocol("MIME is not UTF-8".into()))?;
    assert!(text.ends_with("one\r\ntwo\r\nthree\r\nfour"));
    assert!(
        build_mime_message(
            "sender@example.invalid",
            &["recipient@example.invalid".into()],
            &[],
            &[],
            "bad\r\nBcc: attacker@example.com",
            "body",
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn unsupported_device_policy_is_not_acknowledged_as_supported() {
    for field in ["DevicePasswordEnabled", "RequireStorageCardEncryption"] {
        let policy = BTreeMap::from([(field.into(), "1".into())]);
        let decision = evaluate_policy(&policy);
        assert!(!decision.supported);
        assert!(!decision.reasons.is_empty());
    }
}

#[test]
fn every_device_wide_feature_restriction_is_rejected_with_its_reason() {
    for (field, description) in [
        ("AllowBrowser", "browser restriction"),
        ("AllowCamera", "camera restriction"),
        ("AllowConsumerEmail", "consumer-email restriction"),
        ("AllowDesktopSync", "desktop-sync restriction"),
        ("AllowInternetSharing", "internet-sharing restriction"),
        ("AllowIrDA", "IrDA restriction"),
        ("AllowPOPIMAPEmail", "POP/IMAP restriction"),
        ("AllowRemoteDesktop", "remote-desktop restriction"),
        ("AllowStorageCard", "storage-card restriction"),
        ("AllowTextMessaging", "text-messaging restriction"),
        ("AllowUnsignedApplications", "unsigned-application restriction"),
        ("AllowUnsignedInstallationPackages", "unsigned-package restriction"),
        ("AllowWiFi", "Wi-Fi restriction"),
    ] {
        let policy = BTreeMap::from([(field.into(), "0".into())]);
        let decision = evaluate_policy(&policy);
        assert!(!decision.supported, "{field} restriction was accepted");
        assert_eq!(
            decision.reasons,
            [format!("unsupported device-wide policy: {description}")],
            "unexpected reason for {field}"
        );
    }
}

#[test]
fn policy_rejects_malformed_ranges_and_application_lists() {
    for policy in [
        BTreeMap::from([("MaxEmailAgeFilter".into(), "9".into())]),
        BTreeMap::from([("MaxEmailBodyTruncationSize".into(), "-2".into())]),
        BTreeMap::from([("AttachmentsEnabled".into(), "yes".into())]),
        BTreeMap::from([("ApprovedApplicationList".into(), "1".into())]),
        BTreeMap::from([("UnknownPolicyRequirement".into(), "1".into())]),
    ] {
        assert!(!evaluate_policy(&policy).supported);
    }
}

#[test]
fn policy_applies_server_limits_without_widening_local_defaults() {
    let policy = BTreeMap::from([
        ("MaxAttachmentSize".into(), (50 * 1024 * 1024).to_string()),
        ("MaxEmailAgeFilter".into(), "3".into()),
        ("MaxCalendarAgeFilter".into(), "7".into()),
        ("AllowHTMLEmail".into(), "0".into()),
        ("MaxEmailBodyTruncationSize".into(), "4096".into()),
    ]);
    let decision = evaluate_policy(&policy);
    assert!(decision.supported);
    assert_eq!(decision.max_attachment_bytes, 25 * 1024 * 1024);
    assert_eq!(decision.body_limit, 4096);
    assert_eq!(decision.mail_filter_type, 3);
    assert_eq!(decision.calendar_filter_type, 6);
}

#[test]
fn policy_defaults_and_bluetooth_restrictions_are_exact() {
    let defaults = evaluate_policy(&BTreeMap::new());
    assert!(defaults.supported);
    assert_eq!(defaults.max_attachment_bytes, 25 * 1024 * 1024);

    for (value, supported) in [("0", false), ("1", false), ("2", true), ("invalid", false)] {
        let decision = evaluate_policy(&BTreeMap::from([("AllowBluetooth".into(), value.into())]));
        assert_eq!(decision.supported, supported, "unexpected result for AllowBluetooth={value}");
    }
}

#[test]
fn policy_accepts_empty_optional_numbers_but_rejects_empty_required_scalars() {
    let optional = BTreeMap::from([
        ("DevicePasswordExpiration".into(), String::new()),
        ("MaxAttachmentSize".into(), String::new()),
        ("MaxDevicePasswordFailedAttempts".into(), String::new()),
        ("MaxInactivityTimeDeviceLock".into(), String::new()),
        ("MinDevicePasswordLength".into(), String::new()),
    ]);
    assert!(evaluate_policy(&optional).supported);

    for name in ["AttachmentsEnabled", "MaxEmailBodyTruncationSize"] {
        let malformed = BTreeMap::from([(name.into(), String::new())]);
        assert!(!evaluate_policy(&malformed).supported);
    }
}

#[test]
fn provision_parser_counts_nested_policy_application_entries() -> eas_mail_protocol::Result<()> {
    let mut root = Element::new("Provision", "Provision");
    root.push(Element::text("Provision", "Status", "1"));
    let mut policies = Element::new("Provision", "Policies");
    let mut policy = Element::new("Provision", "Policy");
    policy.push(Element::text("Provision", "Status", "1"));
    policy.push(Element::text("Provision", "PolicyKey", "42"));
    let mut data = Element::new("Provision", "Data");
    let mut document = Element::new("Provision", "EASProvisionDoc");
    let mut approved = Element::new("Provision", "ApprovedApplicationList");
    approved.push(Element::text("Provision", "Hash", "fixture-hash"));
    document.push(approved);
    data.push(document);
    policy.push(data);
    policies.push(policy);
    root.push(policies);

    let parsed = parse_provision(&encode(&root)?)?;
    assert_eq!(parsed.policy.get("ApprovedApplicationList").map(String::as_str), Some("1"));
    assert!(!evaluate_policy(&parsed.policy).supported);
    Ok(())
}
