//! Deterministic EAS and stdio MCP test harness.

#![deny(missing_docs)]

pub mod contract;
mod deterministic;
mod fake_backend;
#[cfg(feature = "live")]
pub mod live_mail;
mod memory_journal;
mod scripted_transport;

pub use deterministic::{FixedClock, ManualClock, SequenceIds};
pub use fake_backend::FakeBackend;
pub use memory_journal::MemoryJournal;
pub use scripted_transport::{ExpectedCall, ScriptedFailure, ScriptedTransport};
