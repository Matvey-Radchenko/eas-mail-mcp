use eas_mail_protocol::{CalendarApplication, CalendarFields, MailFields, ProfileKey};

/// Safe account metadata exposed by a backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendAccount {
    /// Stable local identifier.
    pub account_id: String,
    /// Fixed managed endpoint profile.
    pub profile: ProfileKey,
    /// Mailbox address.
    pub email: String,
    /// Email domains associated with the account profile.
    pub email_domains: Vec<String>,
    /// Whether the account is enabled.
    pub enabled: bool,
    /// Whether mail mutations are enabled.
    pub write_enabled: bool,
}

/// Immutable Exchange reference for one message.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum MailSource {
    /// Item returned by collection Sync.
    Item {
        /// Folder collection identifier.
        folder_id: String,
        /// Message server identifier.
        server_id: String,
    },
    /// Item returned by server-side Search.
    LongId(String),
}

/// Process-local mail record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendMail {
    /// Stable account identifier.
    pub account_id: String,
    /// Folder identifier, empty for Search LongId results.
    pub folder_id: String,
    /// Exchange source reference.
    pub source: MailSource,
    /// Parsed mail fields.
    pub fields: MailFields,
}

/// One bounded server-side mail search page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendMailSearchPage {
    /// Server warned that its retrievable range may omit further matches.
    pub server_truncated: bool,
    /// Candidate messages before local metadata filtering.
    pub items: Vec<BackendMail>,
    /// Optional server estimate of matching candidates.
    pub total: Option<usize>,
    /// Inclusive server range, when supplied.
    pub range: Option<eas_mail_protocol::SearchRange>,
}

/// Process-local calendar record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendEvent {
    /// Original recurrence start selected by a portable occurrence reference.
    pub occurrence_start: Option<chrono::DateTime<chrono::Utc>>,
    /// Stable account identifier.
    pub account_id: String,
    /// Search LongId used for an on-demand ItemOperations fetch.
    pub long_id: String,
    /// Calendar collection identifier when resolved.
    pub collection_id: Option<String>,
    /// Calendar server identifier when resolved.
    pub server_id: Option<String>,
    /// Parsed event fields.
    pub fields: CalendarFields,
}

/// Bounded Calendar Search result from one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendCalendarSearch {
    /// Matching own-calendar events.
    pub events: Vec<BackendEvent>,
    /// Total matches reported by Exchange.
    pub total: usize,
}

/// Optional account capabilities relevant to public tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendCapabilities {
    /// Whether ResolveRecipients Availability is advertised.
    pub calendar_availability: bool,
    /// Whether all mail compose commands are advertised.
    pub mail_writes: bool,
    /// Whether Calendar Add, Change, and Delete are available.
    pub personal_calendar_writes: bool,
    /// Whether meeting notifications and received responses are available.
    pub meeting_lifecycle: bool,
    /// Whether Settings is advertised; write permission is not probed.
    pub auto_reply: bool,
    /// Whether MoveItems is advertised; write permission is not probed.
    pub mail_move: bool,
    /// Whether Sync property changes are advertised; write permission is not probed.
    pub mail_properties: bool,
}

/// Prepared non-recurring event sent to a backend Calendar mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendCalendarMutation {
    /// Optional existing Calendar folder for a newly split series.
    pub target_collection: Option<String>,
    /// Complete EAS Calendar ApplicationData.
    pub application: CalendarApplication,
}

/// One explicit synchronization result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendSync {
    /// Number of collections synchronized.
    pub collections: usize,
    /// Ordered changes applied to process-local state.
    pub changes: usize,
}

/// Plain-text outgoing message accepted by EAS compose commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingMail {
    /// To recipients.
    pub to: Vec<String>,
    /// Cc recipients.
    pub cc: Vec<String>,
    /// Bcc recipients.
    pub bcc: Vec<String>,
    /// Message subject.
    pub subject: String,
    /// Plain-text body.
    pub body: String,
    /// Prepared local attachments held in memory for this operation only.
    pub attachments: Vec<eas_mail_protocol::protocol::MimeAttachment>,
}
