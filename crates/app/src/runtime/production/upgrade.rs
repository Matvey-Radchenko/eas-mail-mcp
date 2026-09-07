mod fixture;

use std::sync::Arc;

use crate::backend::{AccountBackend, MailSource};
use crate::journal::payload_fingerprint;
use crate::references::References;
use crate::{
    ErrorCode, MailSendInput, MemorySecretStore, OperationJournal as _, OperationState,
    OperationStatus, Paths, RandomIds, Runtime, SecretBundle, SecretStore as _, SqliteJournal,
    SystemClock, load_config, load_profile_registry,
};

#[tokio::test]
async fn upgrade_preserves_bundle_references_and_confirmed_uuid_without_resend()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = write_fixture(directory.path(), "succeeded")?;
    let before: SecretBundle = serde_json::from_str(fixture::SECRETS)?;
    let secrets = Arc::new(MemorySecretStore::with_bundle(before.clone()));
    let journal = Arc::new(SqliteJournal::open(&paths.journal)?);
    let runtime = upgraded_runtime(&paths, secrets.clone(), journal.clone())?;
    let input: MailSendInput = serde_json::from_str(fixture::CANONICAL_SEND)?;
    let result = runtime.mail_send(input.clone()).await;
    assert!(result.error.is_none());
    assert_eq!(result.data.map(|value| value.status), Some(OperationState::Succeeded));
    assert_eq!(
        runtime.mail_send(input).await.data.map(|value| value.status),
        Some(OperationState::Succeeded)
    );
    let record =
        journal.inspect(fixture::UUID)?.ok_or_else(|| anyhow::anyhow!("legacy UUID missing"))?;
    assert_eq!(record.record.payload_hmac, fixture::HMAC);
    assert_eq!(record.record.status, OperationStatus::Succeeded);
    assert_eq!((record.created_at, record.updated_at), (1_788_480_000, 1_788_480_001));
    assert!(record.result_locator.is_none());
    assert_eq!(journal.list(&crate::JournalFilter::default())?.len(), 1);
    assert_preserved(&paths, &before, &secrets)?;
    assert_legacy_references()?;
    let connection = rusqlite::Connection::open(&paths.journal)?;
    let version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    assert_eq!(version, 1);
    Ok(())
}

#[tokio::test]
async fn legacy_ambiguous_and_failed_uuids_never_become_new_operations() -> anyhow::Result<()> {
    for (stored, expected) in [
        ("pending", OperationState::Unknown),
        ("unknown", OperationState::Unknown),
        ("failed", OperationState::Failed),
    ] {
        let directory = tempfile::tempdir()?;
        let paths = write_fixture(directory.path(), stored)?;
        let secrets =
            Arc::new(MemorySecretStore::with_bundle(serde_json::from_str(fixture::SECRETS)?));
        let journal = Arc::new(SqliteJournal::open(&paths.journal)?);
        let runtime = upgraded_runtime(&paths, secrets, journal.clone())?;
        let input: MailSendInput = serde_json::from_str(fixture::CANONICAL_SEND)?;
        let response = runtime.mail_send(input.clone()).await;
        assert!(response.error.is_none());
        assert_eq!(response.data.map(|value| value.status), Some(expected));
        let mut changed = input;
        changed.body.push_str(" changed");
        assert_eq!(
            runtime.mail_send(changed).await.error.map(|error| error.code),
            Some(ErrorCode::IdempotencyConflict)
        );
        assert_eq!(journal.list(&crate::JournalFilter::default())?.len(), 1);
    }
    Ok(())
}

fn upgraded_runtime(
    paths: &Paths,
    secrets: Arc<MemorySecretStore>,
    journal: Arc<SqliteJournal>,
) -> anyhow::Result<Runtime> {
    let config = load_config(&paths.config)?;
    let profiles = load_profile_registry(&paths.profiles)?
        .ok_or_else(|| anyhow::anyhow!("profiles missing"))?;
    config.validate_profiles(&profiles)?;
    let account = config.accounts.get("work").ok_or_else(|| anyhow::anyhow!("account missing"))?;
    let bundle = secrets.load()?;
    let configured = super::configured_backend(
        "work".into(),
        account.clone(),
        secrets.clone(),
        bundle.accounts.get("work").cloned(),
        &profiles,
    );
    assert!(configured.configuration_error().is_none());
    // Recovery must work with credentials unavailable; this backend cannot send or use network.
    let unavailable: Arc<dyn AccountBackend> = Arc::new(crate::backend::UnavailableBackend::new(
        configured.account(),
        crate::AppError::new(ErrorCode::AuthRequired, "fixture credentials unavailable"),
    ));
    Ok(Runtime::with_dependencies(
        vec![unavailable],
        journal,
        Arc::new(SystemClock),
        Arc::new(RandomIds),
        bundle.hmac_key.clone(),
        paths.attachments.clone(),
    )?)
}

fn write_fixture(root: &std::path::Path, status: &str) -> anyhow::Result<Paths> {
    let paths = Paths {
        support: root.join("support"),
        attachments: root.join("attachments"),
        config: root.join("support/config.toml"),
        profiles: root.join("support/profiles.toml"),
        journal: root.join("support/operations.sqlite"),
    };
    paths.ensure()?;
    std::fs::write(&paths.config, fixture::CONFIG)?;
    std::fs::write(&paths.profiles, fixture::PROFILES)?;
    let connection = rusqlite::Connection::open(&paths.journal)?;
    connection.execute_batch(fixture::SCHEMA)?;
    connection.execute(
        "INSERT INTO operations VALUES (?1, 'work', 'mail_send', ?2, ?1, ?3, 0, 1788480000, 1788480001)",
        rusqlite::params![fixture::UUID, fixture::HMAC, status],
    )?;
    let version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    assert_eq!(version, 0);
    Ok(paths)
}

fn assert_preserved(
    paths: &Paths,
    before: &SecretBundle,
    secrets: &MemorySecretStore,
) -> anyhow::Result<()> {
    assert_eq!(std::fs::read_to_string(&paths.config)?, fixture::CONFIG);
    assert_eq!(std::fs::read_to_string(&paths.profiles)?, fixture::PROFILES);
    let after = secrets.load()?;
    assert!(before.hmac_key == after.hmac_key);
    assert!(before.accounts == after.accounts);
    assert_eq!(after.version, 1);
    assert_eq!(
        after.accounts.get("work").map(|secret| secret.device_id.as_str()),
        Some("0011223344556677")
    );
    assert_eq!(
        payload_fingerprint(&after.hmac_key, fixture::CANONICAL_SEND.as_bytes())?,
        fixture::HMAC
    );
    Ok(())
}

fn assert_legacy_references() -> anyhow::Result<()> {
    let references = References::new(Arc::new(SystemClock), Arc::new(RandomIds));
    let item = references.mail(fixture::MAIL_ITEM)?;
    assert_eq!(item.account_id, "work");
    assert_eq!(
        item.source,
        MailSource::Item { folder_id: "inbox".into(), server_id: "message-1".into() }
    );
    assert_eq!(
        references.mail(fixture::MAIL_SEARCH)?.source,
        MailSource::LongId("legacy-search-1".into())
    );
    let event = references.event(fixture::EVENT)?;
    assert_eq!(event.account_id, "work");
    assert_eq!(event.long_id, "event-1");
    assert_eq!(event.collection_id.as_deref(), Some("calendar"));
    assert_eq!(event.server_id.as_deref(), Some("event-1"));
    assert!(event.occurrence_start.is_none());
    let occurrence = references.event(fixture::OCCURRENCE)?;
    assert_eq!(occurrence.occurrence_start, Some("2026-09-15T10:00:00Z".parse()?));
    Ok(())
}
