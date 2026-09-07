use chrono::{DateTime, Utc};

use crate::SearchMail;

/// Predicates supported by EAS 14.1 Mailbox Search.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailSearchQuery {
    /// Optional full-text terms, interpreted by Exchange.
    pub text: String,
    /// Collections to search; empty searches all mail folders.
    pub folder_ids: Vec<String>,
    /// Exclusive lower bound on the receive timestamp.
    pub received_after: Option<DateTime<Utc>>,
    /// Exclusive upper bound on the receive timestamp.
    pub received_before: Option<DateTime<Utc>>,
    /// Opaque 16-byte Email2 conversation identifier, retained without text conversion.
    pub conversation_id: Option<Vec<u8>>,
}

/// Inclusive zero-based indices reported by a Search response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchRange {
    /// First returned index.
    pub start: usize,
    /// Last returned index.
    pub end: usize,
}

/// One bounded Search response with server coverage metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMailPage {
    /// Server warning 12: the retrievable result range ended and may omit matches.
    pub server_truncated: bool,
    /// Ordered candidate messages.
    pub items: Vec<SearchMail>,
    /// Optional estimated total; absence never means zero.
    pub total: Option<usize>,
    /// Optional server response range.
    pub range: Option<SearchRange>,
}
