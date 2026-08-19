use std::io::{self, BufRead as _, Write as _};
use std::sync::Arc;

use clap::Parser;
use eas_mail_mcp::backend::{AccountBackend, EasMailbox};
use eas_mail_mcp::{
    AccountConfig, AccountSecret, MemorySecretStore, Paths, RandomIds, Runtime, SecretBundle,
    SecretStore, SystemClock, load_profile_registry,
};
use eas_mail_mcp_harness::MemoryJournal;
use eas_mail_protocol::{HttpTransport, ProfileKey, ProfileRegistry};
use zeroize::Zeroizing;

#[path = "live_harness/checks.rs"]
mod checks;
#[path = "live_harness/support.rs"]
mod support;

use checks::check_account;
use support::{Report, confirm};

#[derive(Debug, Parser)]
struct Arguments {
    #[arg(long)]
    profile: ProfileKey,
    #[arg(long)]
    account_id: String,
    #[arg(long)]
    email: String,
    #[arg(long)]
    username: String,
    #[arg(long)]
    password_stdin: bool,
    #[arg(long)]
    self_write: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arguments = Arguments::parse();
    anyhow::ensure!(arguments.password_stdin, "the direct harness requires --password-stdin");
    if arguments.self_write {
        confirm()?;
    }
    let password = read_password()?;
    let paths = Paths::standard()?;
    let profiles = load_profile_registry(&paths.profiles)?
        .ok_or_else(|| anyhow::anyhow!("no endpoint profiles are configured"))?;
    profiles.require(&arguments.profile)?;
    let account = AccountConfig {
        profile: arguments.profile,
        email: arguments.email.clone(),
        username: arguments.username.clone(),
        enabled: true,
        write_enabled: arguments.self_write,
    };
    let (runtime, _temporary) =
        runtime(&arguments.account_id, account, arguments.username, password.as_str(), &profiles)?;
    let report =
        check_account(&runtime, &arguments.account_id, &arguments.email, arguments.self_write)
            .await?;
    serde_json::to_writer_pretty(
        io::stdout(),
        &Report {
            version: env!("CARGO_PKG_VERSION"),
            accounts: vec![report],
            self_write: arguments.self_write,
        },
    )?;
    writeln!(io::stdout())?;
    Ok(())
}

fn runtime(
    account_id: &str,
    account: AccountConfig,
    username: String,
    password: &str,
    profiles: &ProfileRegistry,
) -> anyhow::Result<(Runtime, tempfile::TempDir)> {
    let profile = profiles.require(&account.profile)?;
    let device_id = SecretBundle::device_id(profile.device_id_length())?;
    let mut bundle = SecretBundle::new();
    let hmac_key = bundle.hmac_key.clone();
    bundle.accounts.insert(
        account_id.to_owned(),
        AccountSecret {
            password: password.to_owned(),
            device_id: device_id.clone(),
            policy_key: 0,
            policy: None,
        },
    );
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::with_bundle(bundle));
    let transport = HttpTransport::new(profile, username, password.to_owned(), device_id)?;
    let mailbox = EasMailbox::with_transport(
        account_id.to_owned(),
        account,
        secrets,
        Arc::new(transport),
        0,
        None,
    )?;
    let backend: Arc<dyn AccountBackend> = Arc::new(mailbox);
    let temporary = tempfile::tempdir()?;
    let runtime = Runtime::with_dependencies(
        vec![backend],
        Arc::new(MemoryJournal::default()),
        Arc::new(SystemClock),
        Arc::new(RandomIds),
        hmac_key,
        temporary.path().join("attachments"),
    )?;
    Ok((runtime, temporary))
}

fn read_password() -> anyhow::Result<Zeroizing<String>> {
    let mut password = String::new();
    io::stdin().lock().read_line(&mut password)?;
    let password = password.trim_end_matches(['\r', '\n']).to_owned();
    anyhow::ensure!(!password.is_empty(), "password is empty");
    Ok(Zeroizing::new(password))
}
