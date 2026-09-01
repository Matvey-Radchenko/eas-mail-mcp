mod calendar;
mod calendar_agenda;
mod calendar_binding;
mod calendar_write;
mod calendar_write_model;
mod content;
mod meeting_response;
mod mutations;
mod session;
mod sync;

pub use session::EasMailbox;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerificationStage {
    Profile,
    Transport,
    Capabilities,
    Policy,
    FolderSync,
}

impl VerificationStage {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Profile => "[1/5] Profile and endpoint rules: valid",
            Self::Transport => "[2/5] TLS and authentication: checking",
            Self::Capabilities => "[3/5] EAS 14.1 capabilities: accepted",
            Self::Policy => "[4/5] Provision and policy: checking",
            Self::FolderSync => "[5/5] FolderSync: checking",
        }
    }
}
