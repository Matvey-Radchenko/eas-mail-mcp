//! Exchange ActiveSync protocol and transport primitives.

#![deny(missing_docs)]

mod client;
mod device;
mod error;
mod model;
mod profile;
pub mod protocol;
mod query;
mod transport;
pub mod wbxml;

pub use client::{EasClient, NegotiatedPolicy};
pub use error::{EasError, Result};
pub use model::{
    Attachment, CalendarFields, ChangeData, ChangeKind, CollectionKind, Folder, FolderPage,
    ItemResult, MailFields, MutationResult, Patch, SearchMail, SyncChange, SyncPage,
};
pub use profile::{Profile, ProfileKey, ProfileRegistry};
pub use query::{Command, build_binary_query};
pub use transport::{HttpTransport, RequestSafety, Transport, TransportResponse};
