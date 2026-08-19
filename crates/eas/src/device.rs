pub(crate) const DEVICE_TYPE: &str = "EasMailMCP";

#[cfg(target_os = "macos")]
pub(crate) const OPERATING_SYSTEM: &str = "macOS";

#[cfg(not(target_os = "macos"))]
pub(crate) const OPERATING_SYSTEM: &str = "Unsupported";

pub(crate) fn user_agent(version: &str) -> String {
    format!("{DEVICE_TYPE}/{version} ({OPERATING_SYSTEM})")
}
