mod command;
mod contract;
mod delivery;
mod files;
mod goldens;
mod npm;
mod performance;
mod profile;
mod public_audit;
mod quality;
mod soak;

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "cargo xtask")]
struct Cli {
    #[command(subcommand)]
    command: Task,
}

#[derive(Debug, Subcommand)]
enum Task {
    /// Runs every local quality and security gate.
    Check,
    /// Runs the fast no-retry nextest loop.
    Test,
    /// Verifies or explicitly updates EAS binary goldens.
    Goldens {
        #[arg(value_enum)]
        action: GoldenAction,
    },
    /// Verifies or intentionally updates the semantic MCP and CLI release contract.
    Contract {
        #[arg(value_enum)]
        action: ContractAction,
    },
    /// Checks handwritten Rust file sizes.
    Files,
    /// Scans tracked project text for credentials.
    Secrets,
    /// Validates portable endpoint profile files.
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Audits the tracked tree and Git history for private material.
    PublicAudit {
        /// Additional newline-delimited forbidden terms kept outside public Git.
        #[arg(long)]
        denylist: Option<PathBuf>,
    },
    /// Verifies or builds the public npm package set.
    Npm {
        #[command(subcommand)]
        command: NpmCommand,
    },
    /// Builds generic arm64, x86_64, and dual-architecture handoff bundles.
    BuildBundles,
    /// Runs the real-account read-only or self-write harness.
    Live {
        /// Sends only to the same mailbox after an interactive confirmation.
        #[arg(long)]
        self_write: bool,
    },
    /// Runs the nightly WBXML fuzz targets.
    Fuzz {
        /// Maximum seconds per fuzz target.
        #[arg(long, default_value_t = 60)]
        seconds: u64,
    },
    /// Runs mutation testing for the EAS crate.
    Mutants,
    /// Enforces startup, RSS, binary-size, and Python-baseline performance budgets.
    Perf {
        /// Python executable with benchmarks/requirements.txt installed.
        #[arg(long, default_value = "python3")]
        python: String,
    },
    /// Runs the three-client, two-account read-only acceptance soak.
    Soak {
        /// Duration; release acceptance requires at least eight hours.
        #[arg(long, default_value_t = 8)]
        hours: u64,
        /// Exact downloaded staged executable; omitted only for a local rehearsal.
        #[arg(long)]
        application: Option<PathBuf>,
        /// Save progress and final acceptance evidence as JSON.
        #[arg(long)]
        report: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum GoldenAction {
    Verify,
    Accept,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ContractAction {
    Verify,
    Accept,
    Compatibility,
}

#[derive(Debug, Subcommand)]
enum ProfileCommand {
    /// Validates schema, endpoints, trust paths, PEM, and fingerprints.
    Verify {
        /// Profile bundle; defaults to the public development example.
        #[arg(long, default_value = "profile.example.toml")]
        profile_bundle: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum NpmCommand {
    /// Verifies Cargo/npm versions, platform selectors, and lifecycle safety.
    Verify,
    /// Builds both native binaries and creates installable npm tarballs.
    Pack,
    /// Installs the locally packed host candidate into the global npm prefix.
    InstallCandidate,
}

fn main() -> anyhow::Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow::anyhow!("workspace root is unavailable"))?;
    match Cli::parse().command {
        Task::Check => quality::check(root),
        Task::Test => quality::test(root),
        Task::Goldens { action } => goldens::run(root, matches!(action, GoldenAction::Accept)),
        Task::Contract { action } => match action {
            ContractAction::Verify => contract::run(root, false),
            ContractAction::Accept => contract::run(root, true),
            ContractAction::Compatibility => contract::compatibility(root),
        },
        Task::Files => files::check(root),
        Task::Secrets => quality::secrets(root),
        Task::Profile { command } => match command {
            ProfileCommand::Verify { profile_bundle } => {
                profile::verify(root, &profile_bundle).map(|_| ())
            }
        },
        Task::PublicAudit { denylist } => public_audit::run(root, denylist.as_deref()),
        Task::Npm { command } => match command {
            NpmCommand::Verify => npm::verify(root),
            NpmCommand::Pack => npm::pack(root),
            NpmCommand::InstallCandidate => npm::install_candidate(root),
        },
        Task::BuildBundles => delivery::build(root),
        Task::Live { self_write } => quality::live(root, self_write),
        Task::Fuzz { seconds } => quality::fuzz(root, seconds),
        Task::Mutants => quality::mutants(root),
        Task::Perf { python } => performance::check(root, &python),
        Task::Soak { hours, application, report } => {
            soak::check(root, hours, application.as_deref(), report.as_deref())
        }
    }
}
