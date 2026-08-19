use std::io::{self, BufRead as _, Write as _};
use std::sync::Arc;

use clap::Parser;
use eas_mail_mcp::{Paths, load_profile_registry};
use eas_mail_protocol::{EasClient, HttpTransport, ProfileKey};
use serde::Serialize;
use zeroize::Zeroizing;

#[derive(Debug, Parser)]
struct Arguments {
    #[arg(long)]
    profile: ProfileKey,
    #[arg(long)]
    username: String,
    #[arg(long)]
    password_stdin: bool,
}

#[derive(Serialize)]
struct Report {
    completed_stage: &'static str,
    failed_stage: Option<&'static str>,
    protocol_detail: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arguments = Arguments::parse();
    anyhow::ensure!(arguments.password_stdin, "the live probe requires --password-stdin");
    let password = read_password()?;
    let paths = Paths::standard()?;
    let profiles = load_profile_registry(&paths.profiles)?
        .ok_or_else(|| anyhow::anyhow!("no endpoint profiles are configured"))?;
    let profile = profiles.require(&arguments.profile)?;
    let device_id = "A1".repeat(profile.device_id_length() / 2);
    let transport =
        HttpTransport::new(profile, arguments.username, password.to_string(), device_id)?;
    let client = EasClient::new(Arc::new(transport));

    if let Err(error) = client.options().await {
        return write_report(Report::failed("options", error.to_string()));
    }
    let policy = match client.provision().await {
        Ok(policy) => policy,
        Err(error) => return write_report(Report::failed("provision", error.to_string())),
    };
    if let Err(error) = client.folder_sync(policy.key, "0").await {
        return write_report(Report::failed("folder_sync", error.to_string()));
    }
    write_report(Report::success("folder_sync"))
}

impl Report {
    const fn success(stage: &'static str) -> Self {
        Self { completed_stage: stage, failed_stage: None, protocol_detail: None }
    }

    fn failed(stage: &'static str, detail: String) -> Self {
        Self {
            completed_stage: match stage {
                "options" => "none",
                "provision" => "options",
                _ => "provision",
            },
            failed_stage: Some(stage),
            protocol_detail: Some(detail),
        }
    }
}

fn read_password() -> anyhow::Result<Zeroizing<String>> {
    let mut password = String::new();
    io::stdin().lock().read_line(&mut password)?;
    let password = password.trim_end_matches(['\r', '\n']).to_owned();
    anyhow::ensure!(!password.is_empty(), "password is empty");
    Ok(Zeroizing::new(password))
}

fn write_report(report: Report) -> anyhow::Result<()> {
    serde_json::to_writer_pretty(io::stdout(), &report)?;
    writeln!(io::stdout())?;
    Ok(())
}
