use super::common::{InputSource, WriteControl};
use clap::{Args, ValueEnum};

#[derive(Debug, Args)]
pub(in crate::cli) struct MoveArgs {
    /// Portable message reference.
    pub(super) mail_ref: Option<String>,
    /// Existing destination folder identifier in the same account.
    pub(super) destination_folder_id: Option<String>,
    #[command(flatten)]
    pub(super) source: InputSource,
    #[command(flatten)]
    pub(super) control: WriteControl,
}
#[derive(Debug, Args)]
pub(in crate::cli) struct DeleteArgs {
    /// Portable message reference. Moves to trash, never permanently deletes.
    pub(super) mail_ref: Option<String>,
    #[command(flatten)]
    pub(super) source: InputSource,
    #[command(flatten)]
    pub(super) control: WriteControl,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum FlagArg {
    None,
    Active,
    Complete,
}
#[derive(Debug, Args)]
pub(in crate::cli) struct FlagArgs {
    /// Explicitly load the message's folder before preview; required without prepared Sync state.
    #[arg(long)]
    pub(super) sync_folder: bool,
    /// Portable message reference.
    pub(super) mail_ref: Option<String>,
    /// Desired follow-up state.
    #[arg(value_enum)]
    pub(super) flag: Option<FlagArg>,
    #[command(flatten)]
    pub(super) source: InputSource,
    #[command(flatten)]
    pub(super) control: WriteControl,
}
#[derive(Debug, Args)]
pub(in crate::cli) struct CategoriesArgs {
    /// Explicitly load the message's folder before preview; required without prepared Sync state.
    #[arg(long)]
    pub(super) sync_folder: bool,
    /// Portable message reference.
    pub(super) mail_ref: Option<String>,
    /// New category name; repeat to replace the complete set.
    #[arg(long = "category", conflicts_with = "clear")]
    pub(super) categories: Vec<String>,
    /// Clear all categories.
    #[arg(long)]
    pub(super) clear: bool,
    #[command(flatten)]
    pub(super) source: InputSource,
    #[command(flatten)]
    pub(super) control: WriteControl,
}
#[derive(Debug, Args)]
pub(in crate::cli) struct BatchArgs {
    /// Explicitly load folders of new property changes before preview; moves need no Sync.
    #[arg(long)]
    pub(super) sync_folder: bool,
    /// JSON file or '-' containing items, actions and individual UUIDs.
    #[arg(long)]
    pub(super) input: std::path::PathBuf,
    /// Execute after printing preview without an interactive confirmation.
    #[arg(long)]
    pub(super) yes: bool,
}
#[derive(Debug, Args)]
pub(in crate::cli) struct GetManyArgs {
    /// Portable references, at most 20.
    pub(super) mail_refs: Vec<String>,
    #[command(flatten)]
    pub(super) source: InputSource,
    /// Maximum body characters per message, default 12,000; maximum 50,000.
    #[arg(long)]
    pub(super) body_limit: Option<u32>,
    /// Total body budget, default and maximum 100,000.
    #[arg(long)]
    pub(super) total_body_limit: Option<u32>,
}
