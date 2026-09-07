use super::*;
use crate::{FakeBackend, MemoryJournal};
use eas_mail_mcp::{ErrorCode, MailListInput, RandomIds, SystemClock};
use std::sync::Arc;

fn runtime(backend: Arc<FakeBackend>, dir: &std::path::Path) -> anyhow::Result<Runtime> {
    Ok(Runtime::with_dependencies(
        vec![backend],
        Arc::new(MemoryJournal::default()),
        Arc::new(SystemClock),
        Arc::new(RandomIds),
        vec![9; 32],
        dir.join("attachments"),
    )?)
}

async fn fixture_mail(runtime: &Runtime) -> anyhow::Result<MailDetail> {
    let page = required(runtime.mail_list(MailListInput::default()).await, "fixture list")?;
    let mail = page.items.first().ok_or_else(|| anyhow::anyhow!("no fixture"))?;
    get(runtime, &mail.mail_ref).await
}

#[tokio::test]
async fn arbitrary_existing_message_is_rejected_before_any_write() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let backend = Arc::new(FakeBackend::new("work"));
    let runtime = runtime(backend.clone(), temp.path())?;
    let mail = fixture_mail(&runtime).await?;
    let fixture = SyntheticMailFixture {
        account_id: "work".into(),
        inbox_id: "inbox".into(),
        trash_id: "trash".into(),
        started_at: Utc::now(),
        messages: [
            SyntheticMail {
                mail_ref: mail.summary.mail_ref,
                subject: format!("EAS Mail MCP self-test {}", uuid::Uuid::new_v4()),
            },
            SyntheticMail {
                mail_ref: "unused".into(),
                subject: format!("EAS Mail MCP self-test {}", uuid::Uuid::new_v4()),
            },
        ],
    };
    let error = check_synthetic_mail(&runtime, &fixture).await.err();
    assert!(error.is_some_and(|error| error.to_string().contains("provenance")));
    assert!(backend.operations()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn properties_and_batch_restore_confirmed_fixture_state() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let backend = Arc::new(FakeBackend::new("work"));
    let runtime = runtime(backend.clone(), temp.path())?;
    let mail = fixture_mail(&runtime).await?;
    writes::properties(&runtime, &mail).await?;
    writes::batch(&runtime, std::slice::from_ref(&mail)).await?;
    let final_mail = get(&runtime, &mail.summary.mail_ref).await?;
    assert_eq!(final_mail.summary.is_read, mail.summary.is_read);
    assert_eq!(final_mail.summary.categories, Some(Vec::new()));
    assert_eq!(final_mail.summary.flag, Some(eas_mail_mcp::MailFlagState::None));
    assert_eq!(backend.operations()?.len(), 7);
    Ok(())
}

#[tokio::test]
async fn unknown_flag_stops_without_restore_or_later_mutations() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let backend = Arc::new(FakeBackend::new("work"));
    let runtime = runtime(backend.clone(), temp.path())?;
    let mail = fixture_mail(&runtime).await?;
    backend.set_operation_failure(Some("mail_set_flag"), ErrorCode::OutcomeUnknown)?;
    let error = writes::properties(&runtime, &mail).await.err();
    assert!(error.is_some_and(|error| error.to_string().contains("OutcomeUnknown")));
    assert!(backend.operations()?.is_empty());
    Ok(())
}
