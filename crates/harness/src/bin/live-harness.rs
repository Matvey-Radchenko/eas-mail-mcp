use std::io::{self, Write as _};
use std::sync::Arc;

use clap::Parser;
use eas_mail_mcp::{Paths, Runtime, load_config, load_profile_registry};

#[path = "live_harness/calendar_lifecycle.rs"]
mod calendar_lifecycle;
#[path = "live_harness/checks.rs"]
mod checks;
#[path = "live_harness/support.rs"]
mod support;
#[path = "live_harness/write_outcome.rs"]
mod write_outcome;

use calendar_lifecycle::{LiveAccount, MeetingCoverage};
use checks::check_account;
use support::{Report, confirm};

#[derive(Debug, Parser)]
struct Arguments {
    #[arg(long)]
    self_write: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arguments = Arguments::parse();
    if arguments.self_write {
        confirm()?;
    }
    let paths = Paths::standard()?;
    let profiles = load_profile_registry(&paths.profiles)?
        .ok_or_else(|| anyhow::anyhow!("no endpoint profiles are configured"))?;
    let config = load_config(&paths.config)?;
    let runtime = Arc::new(Runtime::production(config.clone(), &paths, &profiles)?);
    let mut reports = Vec::new();
    let accounts = config
        .accounts
        .into_iter()
        .filter(|(_, account)| account.enabled)
        .map(|(account_id, account)| LiveAccount {
            account_id,
            profile: account.profile.to_string(),
            email: account.email,
            write_enabled: account.write_enabled,
        })
        .collect::<Vec<_>>();
    for account in &accounts {
        let mut report =
            check_account(&runtime, &account.account_id, &account.email, arguments.self_write)
                .await?;
        if arguments.self_write {
            calendar_lifecycle::check_personal_events(&runtime, &account.account_id).await?;
            report.calendar_writes_checked = true;
        }
        reports.push(report);
    }
    anyhow::ensure!(!reports.is_empty(), "no enabled accounts are configured");
    let meeting_coverage = if arguments.self_write {
        calendar_lifecycle::check_meeting_directions(&runtime, &accounts).await?
    } else {
        MeetingCoverage::default()
    };
    if arguments.self_write && meeting_coverage.profiles == 0 {
        writeln!(
            io::stderr(),
            "Calendar meeting lifecycle was not run: no endpoint profile has two writable accounts."
        )?;
    }
    serde_json::to_writer_pretty(
        io::stdout(),
        &Report {
            version: env!("CARGO_PKG_VERSION"),
            accounts: reports,
            self_write: arguments.self_write,
            meeting_profiles: meeting_coverage.profiles,
            meeting_directions: meeting_coverage.directions,
        },
    )?;
    writeln!(io::stdout())?;
    Ok(())
}
