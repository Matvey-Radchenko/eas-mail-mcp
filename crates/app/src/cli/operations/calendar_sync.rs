use super::common::InputSource;
use super::input::{ensure_flag_mode, read_json, required};
use crate::{CalendarSyncInput, Result};
use clap::Args;

#[derive(Debug, Args)]
pub(in crate::cli) struct CalendarSyncArgs {
    #[command(flatten)]
    source: InputSource,
    /// Account whose calendar state should be synchronized.
    #[arg(long)]
    account: Option<String>,
    /// Optional calendar collection identifier.
    #[arg(long)]
    collection_id: Option<String>,
    /// Optional expected stored key; normally omitted. Never use an unrelated key.
    #[arg(long, conflicts_with = "full")]
    sync_key: Option<String>,
    /// Rebuild the selected baselines while retaining old available data.
    #[arg(long)]
    full: bool,
    /// First local display date; default today minus 90 days.
    #[arg(long = "from")]
    date_from: Option<String>,
    /// Last local display date; default today plus 90 days.
    #[arg(long = "to")]
    date_to: Option<String>,
    /// IANA timezone; default UTC.
    #[arg(long)]
    time_zone: Option<String>,
    /// Sync pages per collection; default 20, maximum 100.
    #[arg(long)]
    max_pages: Option<u16>,
}

impl CalendarSyncArgs {
    pub(super) fn into_input(self) -> Result<CalendarSyncInput> {
        ensure_flag_mode(
            self.source.input.as_ref(),
            self.account.is_some()
                || self.collection_id.is_some()
                || self.sync_key.is_some()
                || self.full
                || self.date_from.is_some()
                || self.date_to.is_some()
                || self.time_zone.is_some()
                || self.max_pages.is_some(),
        )?;
        if let Some(path) = self.source.input {
            return read_json(&path);
        }
        Ok(CalendarSyncInput {
            account_id: required(self.account, "account")?,
            collection_id: self.collection_id,
            sync_key: self.sync_key,
            full: self.full,
            date_from: self.date_from,
            date_to: self.date_to,
            time_zone: self.time_zone,
            max_pages: self.max_pages,
        })
    }
}
