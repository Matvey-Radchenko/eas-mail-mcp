//! Calendar-only acceptance using fresh personal fixtures, with no failure cleanup or retries.
use std::io::{self, Write as _};

use anyhow::{Context as _, Result};
use clap::Parser;
use eas_mail_mcp::{Paths, Runtime, load_config, load_profile_registry};
use serde_json::{Value, json};

#[path = "calendar_live/fixtures.rs"]
mod fixtures;
#[path = "calendar_live/slots.rs"]
mod slots;
#[path = "calendar_live/wire.rs"]
mod wire;

#[derive(Parser)]
struct Arguments {
    /// Limit validation to one configured account, preserving its configuration ordinal in reports.
    #[arg(long)]
    account: Option<String>,
    /// Record only numeric protocol statuses and tag paths through the standard HTTPS transport.
    #[arg(long)]
    wire_status: bool,
    /// Resume only a recent journal-confirmed synthetic series create and definite failed delete.
    #[arg(long, requires_all = ["account", "self_write", "failed_delete"])]
    resume_series: Option<String>,
    /// Exact prior delete UUID; pending, partial, or unknown states are refused.
    #[arg(long, requires = "resume_series")]
    failed_delete: Option<String>,
    /// Also verify an existing server-expanded exception can be edited again before removal.
    #[arg(long, requires = "resume_series")]
    exercise_existing_exception: bool,
    /// Permit new personal calendar fixtures. Never adds attendees or sends meeting mail.
    #[arg(long)]
    self_write: bool,
    /// Run only the fresh weekly personal fixture, for focused lifecycle validation.
    #[arg(long, requires = "self_write")]
    recurring_only: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let arguments = Arguments::parse();
    let paths = Paths::standard()?;
    let profiles = load_profile_registry(&paths.profiles)?.context("profiles unavailable")?;
    let config = load_config(&paths.config)?;
    let runtime = if arguments.wire_status {
        wire::runtime(&config, &paths, &profiles, arguments.account.as_deref())?
    } else {
        Runtime::production(config.clone(), &paths, &profiles)?
    };
    if let Some(created) = &arguments.resume_series {
        return fixtures::resume::run(
            &runtime,
            &paths,
            arguments.account.as_deref().context("resume account is required")?,
            created,
            arguments.failed_delete.as_deref().context("failed delete is required")?,
            arguments.exercise_existing_exception,
        )
        .await;
    }
    let accounts = config.accounts.iter().enumerate().filter(|(_, (id, account))| {
        account.enabled && arguments.account.as_ref().is_none_or(|selected| selected == *id)
    });
    let mut count = 0;
    for (index, (id, account)) in accounts {
        count += 1;
        if !arguments.recurring_only {
            let coverage = slots::check(&runtime, id, &account.email).await?;
            report(json!({"account_index": index + 1, "stage": "slots", "coverage": coverage}))?;
        }
        if arguments.self_write {
            anyhow::ensure!(account.write_enabled, "account does not permit calendar writes");
            for (kind, stage) in [(false, "personal_timed"), (true, "personal_all_day")] {
                if arguments.recurring_only {
                    break;
                }
                fixtures::personal(&runtime, id, kind).await?;
                report(json!({"account_index": index + 1, "stage": stage, "round_trip": true,
                    "deleted": true, "attendees": 0}))?;
            }
            fixtures::recurring(&runtime, id).await?;
            report(json!({"account_index": index + 1, "stage": "personal_weekly",
                "occurrence_update": true, "occurrence_delete": true, "series_deleted": true,
                "attendees": 0}))?;
        }
    }
    anyhow::ensure!(count > 0, "no enabled accounts");
    Ok(())
}

fn report(value: Value) -> Result<()> {
    serde_json::to_writer(io::stdout().lock(), &value)?;
    writeln!(io::stdout().lock())?;
    Ok(())
}
