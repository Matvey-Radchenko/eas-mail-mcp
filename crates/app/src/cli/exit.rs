/// Process exit category returned after a successfully parsed CLI command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliExit {
    /// The command completed successfully.
    Success,
    /// An explicit diagnostic check found an unhealthy configuration or account.
    Unhealthy,
    /// The user declined an interactive mutation.
    Declined,
    /// Exchange returned a failed, partial, or unknown mutation outcome.
    WriteNotSucceeded,
}

impl CliExit {
    /// Returns the documented process exit code.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Unhealthy => 1,
            Self::Declined => 2,
            Self::WriteNotSucceeded => 3,
        }
    }
}
