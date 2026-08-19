use std::collections::BTreeMap;

use crate::device::OPERATING_SYSTEM;
use crate::wbxml::{decode, encode};
use crate::{EasError, Result};

use super::tree::{element, integer, push_text};

/// Parsed Provision response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionResult {
    /// Top-level Provision status.
    pub status: u16,
    /// Policy acknowledgement status.
    pub policy_status: Option<u16>,
    /// Temporary or final policy key.
    pub policy_key: Option<u32>,
    /// Raw EASProvisionDoc scalar fields.
    pub policy: BTreeMap<String, String>,
    /// Device-wide wipe directive.
    pub remote_wipe: bool,
    /// Application-data-only wipe directive.
    pub account_only_remote_wipe: bool,
}

/// Builds the first Provision request with honest device information.
pub fn build_initial_provision() -> Result<Vec<u8>> {
    let mut root = element("Provision", "Provision");
    let mut information = element("Settings", "DeviceInformation");
    let mut set = element("Settings", "Set");
    push_text(&mut set, "Settings", "Model", "EAS Mail MCP");
    push_text(&mut set, "Settings", "FriendlyName", "EAS Mail MCP");
    push_text(&mut set, "Settings", "OS", OPERATING_SYSTEM);
    push_text(&mut set, "Settings", "OSLanguage", "en");
    information.push(set);
    root.push(information);
    let mut policies = element("Provision", "Policies");
    let mut policy = element("Provision", "Policy");
    push_text(&mut policy, "Provision", "PolicyType", "MS-EAS-Provisioning-WBXML");
    policies.push(policy);
    root.push(policies);
    encode(&root)
}

/// Builds a supported or rejected policy acknowledgement.
pub fn build_policy_ack(policy_key: u32, supported: bool) -> Result<Vec<u8>> {
    let mut root = element("Provision", "Provision");
    let mut policies = element("Provision", "Policies");
    let mut policy = element("Provision", "Policy");
    push_text(&mut policy, "Provision", "PolicyType", "MS-EAS-Provisioning-WBXML");
    push_text(&mut policy, "Provision", "PolicyKey", policy_key.to_string());
    push_text(&mut policy, "Provision", "Status", if supported { "1" } else { "2" });
    policies.push(policy);
    root.push(policies);
    encode(&root)
}

/// Builds a device-wide or account-only remote wipe acknowledgement.
pub fn build_wipe_ack(account_only: bool) -> Result<Vec<u8>> {
    let mut root = element("Provision", "Provision");
    let name = if account_only { "AccountOnlyRemoteWipe" } else { "RemoteWipe" };
    let mut wipe = element("Provision", name);
    push_text(&mut wipe, "Provision", "Status", "1");
    root.push(wipe);
    encode(&root)
}

/// Parses policy data and remote-wipe directives.
pub fn parse_provision(data: &[u8]) -> Result<ProvisionResult> {
    let root = decode(data)?
        .ok_or_else(|| EasError::Protocol("Exchange returned an empty Provision".into()))?;
    let status =
        root.child("Provision", "Status").map_or(0, |value| integer(Some(value.text_content()), 0));
    let policy_element = root.descendant("Provision", "Policy");
    let policy_status = policy_element
        .and_then(|value| value.child("Provision", "Status"))
        .map(|value| integer(Some(value.text_content()), 0));
    let policy_key = policy_element
        .and_then(|value| value.child("Provision", "PolicyKey"))
        .and_then(|value| value.text_content().parse().ok());
    let policy = root
        .descendant("Provision", "EASProvisionDoc")
        .map(|document| document.children().map(policy_value).collect())
        .unwrap_or_default();
    Ok(ProvisionResult {
        status,
        policy_status,
        policy_key,
        policy,
        remote_wipe: root.descendant("Provision", "RemoteWipe").is_some(),
        account_only_remote_wipe: root.descendant("Provision", "AccountOnlyRemoteWipe").is_some(),
    })
}

fn policy_value(child: &crate::wbxml::Element) -> (String, String) {
    let value = if matches!(
        child.name.as_str(),
        "ApprovedApplicationList" | "UnapprovedInROMApplicationList"
    ) {
        child.children().count().to_string()
    } else {
        child.text_content()
    };
    (child.name.clone(), value)
}
