use super::*;
use crate::cli::terminal::testing::ScriptedTerminal;

#[test]
fn declining_clear_preserves_downloads() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = fixture_paths(directory.path());
    let cache = AttachmentCache::new(paths.attachments.clone(), Arc::new(SystemClock))?;
    let (download, _) = cache.store("work", "one", "report.txt", b"private attachment")?;
    let mut terminal = ScriptedTerminal::new(&["no"], &[]);
    let outcome = run(&paths, CacheCommand::Clear { account: None, yes: false }, &mut terminal)?;
    assert_eq!(outcome, CliExit::Declined);
    assert!(download.exists());
    Ok(())
}

#[test]
fn clear_requires_confirmation_and_rejects_invalid_account_ids() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = fixture_paths(directory.path());
    let mut terminal = ScriptedTerminal::new(&[], &[]);
    let error = run(&paths, CacheCommand::Clear { account: None, yes: false }, &mut terminal)
        .err()
        .ok_or_else(|| anyhow::anyhow!("missing confirmation was accepted"))?;
    assert_eq!(error.envelope.code, ErrorCode::InteractiveRequired);
    let error = run(
        &paths,
        CacheCommand::Clear { account: Some("../other".into()), yes: true },
        &mut terminal,
    )
    .err()
    .ok_or_else(|| anyhow::anyhow!("invalid account was accepted"))?;
    assert_eq!(error.envelope.code, ErrorCode::ValidationFailed);
    Ok(())
}

fn fixture_paths(directory: &std::path::Path) -> Paths {
    Paths {
        support: directory.join("support"),
        attachments: directory.join("attachments"),
        config: directory.join("support/config.toml"),
        profiles: directory.join("support/profiles.toml"),
        journal: directory.join("support/operations.sqlite"),
    }
}
