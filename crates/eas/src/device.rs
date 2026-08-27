pub(crate) const DEVICE_TYPE: &str = "EasMailMCP";

#[cfg(target_os = "macos")]
pub(crate) const OPERATING_SYSTEM: &str = "macOS";

#[cfg(windows)]
pub(crate) const OPERATING_SYSTEM: &str = "Windows";

#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) const OPERATING_SYSTEM: &str = "Unsupported";

pub(crate) fn user_agent(version: &str) -> String {
    format!("{DEVICE_TYPE}/{version} ({OPERATING_SYSTEM})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_reports_the_supported_operating_system() {
        assert!(!user_agent("1.0.0").contains("Unsupported"));
    }
}
