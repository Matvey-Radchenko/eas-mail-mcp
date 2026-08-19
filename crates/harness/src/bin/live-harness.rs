use std::io::{self, Write as _};
use std::sync::Arc;

use clap::Parser;
use eas_mail_mcp::{Paths, Runtime, load_config, load_profile_registry};

#[path = "live_harness/checks.rs"]
mod checks;
#[path = "live_harness/support.rs"]
mod support;

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
    for (account_id, account) in config.accounts.into_iter().filter(|(_, account)| account.enabled)
    {
        reports.push(
            check_account(&runtime, &account_id, &account.email, arguments.self_write).await?,
        );
    }
    anyhow::ensure!(!reports.is_empty(), "no enabled accounts are configured");
    serde_json::to_writer_pretty(
        io::stdout(),
        &Report {
            version: env!("CARGO_PKG_VERSION"),
            accounts: reports,
            self_write: arguments.self_write,
        },
    )?;
    writeln!(io::stdout())?;
    Ok(())
}
