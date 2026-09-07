use chrono::{DateTime, Utc};

/// Global out-of-office state defined by Settings/OofState.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OofState {
    /// Automatic replies are disabled.
    Disabled,
    /// Automatic replies remain enabled until changed.
    Enabled,
    /// Automatic replies are enabled during an explicit UTC interval.
    Scheduled,
}

/// Audience represented by one Settings/OofMessage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OofAudience {
    /// Senders in the same organization.
    Internal,
    /// External senders represented in the mailbox owner's contacts.
    ExternalKnown,
    /// Other external senders.
    ExternalUnknown,
}

/// One audience's out-of-office reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OofMessage {
    /// Audience selector.
    pub audience: OofAudience,
    /// Whether replies to this audience are enabled.
    pub enabled: bool,
    /// Reply content; absence preserves the existing message in a Set request.
    pub message: Option<String>,
    /// Whether the server returned HTML content instead of requested plain text.
    pub is_html: bool,
}

/// EAS out-of-office settings; messages are not persisted by the application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OofSettings {
    /// Global automatic-reply state.
    pub state: OofState,
    /// Scheduled start, required when state is Scheduled.
    pub starts_at: Option<DateTime<Utc>>,
    /// Scheduled end, required when state is Scheduled.
    pub ends_at: Option<DateTime<Utc>>,
    /// Zero to three audience-specific settings.
    pub messages: Vec<OofMessage>,
}
