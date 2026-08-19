use base64::Engine as _;

use crate::device::DEVICE_TYPE;
use crate::{EasError, Result};

/// Supported ActiveSync commands and compact-query command codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Synchronize a collection.
    Sync,
    /// Send a new message.
    SendMail,
    /// Forward a message.
    SmartForward,
    /// Reply to a message.
    SmartReply,
    /// Synchronize folder hierarchy.
    FolderSync,
    /// Search the mailbox.
    Search,
    /// Fetch an item or attachment.
    ItemOperations,
    /// Negotiate an EAS policy.
    Provision,
}

impl Command {
    const fn code(self) -> u8 {
        match self {
            Self::Sync => 0x00,
            Self::SendMail => 0x01,
            Self::SmartForward => 0x02,
            Self::SmartReply => 0x03,
            Self::FolderSync => 0x09,
            Self::Search => 0x10,
            Self::ItemOperations => 0x13,
            Self::Provision => 0x14,
        }
    }
}

/// Builds the base64 compact query used by Exchange ActiveSync 14.1.
pub fn build_binary_query(
    command: Command,
    device_id: &str,
    policy_key: u32,
    omit_policy_key: bool,
) -> Result<String> {
    if device_id.is_empty()
        || device_id.len() > 32
        || !device_id.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(EasError::InvalidConfiguration(
            "DeviceId must contain 1-32 ASCII letters or digits".into(),
        ));
    }
    let mut payload = vec![141, command.code(), 0x09, 0x04, device_id.len() as u8];
    payload.extend_from_slice(device_id.as_bytes());
    if omit_policy_key {
        payload.push(0);
    } else {
        payload.push(4);
        payload.extend_from_slice(&policy_key.to_le_bytes());
    }
    payload.push(DEVICE_TYPE.len() as u8);
    payload.extend_from_slice(DEVICE_TYPE.as_bytes());
    Ok(base64::engine::general_purpose::STANDARD.encode(payload))
}
