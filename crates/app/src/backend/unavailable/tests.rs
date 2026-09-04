use super::*;
use crate::ErrorCode;
use eas_mail_protocol::{MailSearchQuery, MeetingResponseChoice, OofSettings, OofState};

#[tokio::test]
async fn unavailable_account_keeps_the_same_scoped_error_across_read_families() -> anyhow::Result<()>
{
    let backend = backend();
    let source = MailSource::LongId("private-mail-locator".into());
    let event = event();
    let now = chrono::DateTime::UNIX_EPOCH;
    let results = [
        backend.sync_mail().await.map(|_| ()),
        backend.list_mail(None).await.map(|_| ()),
        backend.search_mail("private-query", 10).await.map(|_| ()),
        backend.search_mail_page(&MailSearchQuery::default(), 0, 10).await.map(|_| ()),
        backend.search_people("private-person", 10).await.map(|_| ()),
        backend.fetch_mail(&source, 1).await.map(|_| ()),
        backend.fetch_attachment("private-attachment").await.map(|_| ()),
        backend
            .calendar_availability(&["private@example.invalid".into()], now, now)
            .await
            .map(|_| ()),
        backend.search_calendar("private-event", 10).await.map(|_| ()),
        backend.scan_calendar_metadata().await.map(|_| ()),
        backend.fetch_calendar(&event, 1).await.map(|_| ()),
        backend.resolve_calendar_source(&event).await.map(|_| ()),
        backend.get_auto_reply().await.map(|_| ()),
    ];
    assert_original_errors(&backend, results)?;
    Ok(())
}

#[tokio::test]
async fn unavailable_account_rejects_writes_before_reading_or_echoing_payloads()
-> anyhow::Result<()> {
    let backend = backend();
    let source = MailSource::LongId("private-mail-locator".into());
    let event = event();
    let message = OutgoingMail {
        to: vec!["private@example.invalid".into()],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: "private-subject".into(),
        body: "private-body".into(),
        attachments: Vec::new(),
    };
    let settings = OofSettings {
        state: OofState::Disabled,
        starts_at: None,
        ends_at: None,
        messages: Vec::new(),
    };
    let results = [
        backend.check_mail_property_ready(&source).await,
        backend.mark_read(&source, true).await,
        backend.move_mail(&source, "private-folder").await.map(|_| ()),
        backend.set_mail_flag(&source, 2).await,
        backend.set_mail_categories(&source, &["private-category".into()]).await,
        backend.send("client", &message).await,
        backend.reply("client", &source, &message).await,
        backend.forward("client", &source, &message).await,
        backend.send_calendar_message("client", b"private-mime".to_vec()).await,
        backend.delete_calendar_item(&event).await,
        backend.respond_calendar_item(&event, MeetingResponseChoice::Accept).await.map(|_| ()),
        backend.respond_meeting_request(&source, MeetingResponseChoice::Accept).await.map(|_| ()),
        backend.set_auto_reply(&settings).await,
    ];
    assert_original_errors(&backend, results)?;
    Ok(())
}

fn assert_original_errors<const N: usize>(
    backend: &UnavailableBackend,
    results: [Result<()>; N],
) -> anyhow::Result<()> {
    let original = backend
        .configuration_error()
        .ok_or_else(|| anyhow::anyhow!("configuration error missing"))?;
    let expected = serde_json::to_value(original)?;
    for result in results {
        let error = result
            .err()
            .ok_or_else(|| anyhow::anyhow!("unavailable backend accepted an operation"))?;
        let actual = serde_json::to_value(error.envelope)?;
        assert_eq!(actual, expected);
        assert!(!actual.to_string().contains("private-"));
        assert!(!actual.to_string().contains("private@"));
    }
    Ok(())
}

fn backend() -> UnavailableBackend {
    // No transport or SecretStore is constructed: the failure boundary cannot contact either.
    UnavailableBackend::new(
        BackendAccount {
            account_id: "work".into(),
            profile: eas_mail_protocol::ProfileKey::default(),
            email: "user@example.invalid".into(),
            email_domains: vec!["example.invalid".into()],
            enabled: true,
            write_enabled: true,
        },
        AppError::new(ErrorCode::AuthRequired, "account credentials are missing")
            .account("work")
            .remediation("Update the account credentials"),
    )
}

fn event() -> BackendEvent {
    BackendEvent {
        occurrence_start: None,
        account_id: "work".into(),
        long_id: "private-event-locator".into(),
        collection_id: None,
        server_id: None,
        fields: eas_mail_protocol::CalendarFields::default(),
    }
}
