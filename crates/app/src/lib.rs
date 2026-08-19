//! Direct stdio MCP application for locally managed EAS mailboxes.

#![deny(missing_docs)]

mod attachment_cache;
pub mod backend;
/// User-facing command-line setup, diagnostics, and client registration.
pub mod cli;
mod config;
mod error;
mod journal;
mod keychain;
/// Official rmcp stdio server and tool routing.
pub mod mcp;
mod model;
mod platform;
mod profiles;
mod references;
mod runtime;
mod sanitize;

pub use config::{AccountConfig, AppConfig, Paths, load_config, save_config};
pub use error::{AppError, ErrorCode, ErrorEnvelope, Result};
pub use journal::{JournalBegin, JournalRecord, OperationJournal, OperationStatus, SqliteJournal};
pub use keychain::{
    AccountSecret, KeychainStore, MemorySecretStore, SecretBundle, SecretStore, StoredPolicy,
};
pub use model::*;
pub use profiles::{load_profile_bundle, load_profile_registry};
pub use references::{Clock, IdGenerator, RandomIds, SystemClock};
pub use runtime::Runtime;
