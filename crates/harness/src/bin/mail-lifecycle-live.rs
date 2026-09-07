//! Explicit self-addressed mail acceptance; all mutating locators originate in this run.
use std::io::{self, Write as _};

use anyhow::{Context as _, Result};
use chrono::Utc;
use clap::Parser;
use eas_mail_mcp::{
    AccountSelection, ApiResponse, AttachmentDownloadInput, MailAttachmentsInput, MailForwardInput,
    MailListInput, MailReplyInput, MailSendInput, OperationResult, OperationState,
    OutgoingAttachmentInput, Paths, Runtime, load_config, load_profile_registry,
};
use eas_mail_mcp_harness::live_mail::{SyntheticMail, SyntheticMailFixture, check_synthetic_mail};
use serde_json::json;

#[derive(Parser)]
struct Arguments {
    /// Explicitly permit fresh self-addressed fixtures and their property/move checks.
    #[arg(long, required = true)]
    self_write: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let arguments = Arguments::parse();
    anyhow::ensure!(arguments.self_write, "self-write authorization is required");
    let paths = Paths::standard()?;
    let profiles = load_profile_registry(&paths.profiles)?.context("profiles are unavailable")?;
    let config = load_config(&paths.config)?;
    anyhow::ensure!(config.accounts.values().any(|account| account.enabled), "no enabled accounts");
    let runtime = Runtime::production(config.clone(), &paths, &profiles)?;
    let temporary = tempfile::tempdir()?;
    let file = temporary.path().join("acceptance.bin");
    let bytes = (0u8..=255).cycle().take(4096).collect::<Vec<_>>();
    std::fs::write(&file, &bytes)?;
    let attachment = OutgoingAttachmentInput {
        path: file.to_string_lossy().into_owned(),
        filename: None,
        content_type: Some("application/octet-stream".into()),
    };
    for (index, (account_id, account)) in
        config.accounts.iter().filter(|(_, account)| account.enabled).enumerate()
    {
        anyhow::ensure!(account.write_enabled, "account does not allow self-write acceptance");
        let folders = required(
            runtime
                .folders_list(AccountSelection { account_ids: Some(vec![account_id.clone()]) })
                .await,
        )?;
        let inbox_id = folders
            .folders
            .iter()
            .find(|folder| folder.role == "inbox")
            .context("no system Inbox")?
            .folder_id
            .clone();
        let trash_id = folders
            .folders
            .iter()
            .find(|folder| folder.role == "trash")
            .context("no system Trash")?
            .folder_id
            .clone();
        let started_at =
            chrono::DateTime::from_timestamp(Utc::now().timestamp(), 0).context("invalid time")?;
        let first =
            send_fixture(&runtime, account_id, &account.email, &inbox_id, &attachment).await?;
        let second =
            send_fixture(&runtime, account_id, &account.email, &inbox_id, &attachment).await?;
        verify_attachment(&runtime, &first.mail_ref, "acceptance.bin", &bytes).await?;
        let fixture = SyntheticMailFixture {
            account_id: account_id.clone(),
            inbox_id,
            trash_id,
            started_at,
            messages: [first, second],
        };
        reply_forward(&runtime, &fixture, &account.email, &attachment, &bytes).await?;
        let coverage = check_synthetic_mail(&runtime, &fixture).await?;
        serde_json::to_writer(
            io::stdout().lock(),
            &json!({"account_index":index+1,"version":env!("CARGO_PKG_VERSION"),"binary_attachment_verified":true,"reply_forward_confirmed":true,"reply_forward_attachments_verified":true,"mail":coverage}),
        )?;
        writeln!(io::stdout().lock())?;
    }
    Ok(())
}

async fn send_fixture(
    runtime: &Runtime,
    account: &str,
    email: &str,
    inbox: &str,
    attachment: &OutgoingAttachmentInput,
) -> Result<SyntheticMail> {
    let subject = format!("EAS Mail MCP self-test {}", uuid::Uuid::new_v4());
    confirmed(runtime.mail_send(MailSendInput {
        account_id: account.into(), to: vec![email.into()], cc: Vec::new(), bcc: Vec::new(), subject: subject.clone(),
        body: "EAS Mail MCP automated acceptance. Dedicated synthetic message; no reply required.".into(),
        attachments: vec![attachment.clone()], idempotency_key: uuid::Uuid::new_v4().to_string(),
    }).await)?;
    let mail_ref = wait_for_mail(runtime, account, inbox, &subject).await?;
    Ok(SyntheticMail { mail_ref, subject })
}

async fn wait_for_mail(
    runtime: &Runtime,
    account: &str,
    inbox: &str,
    subject: &str,
) -> Result<String> {
    for _ in 0..30 {
        let page = required(
            runtime
                .mail_list(MailListInput {
                    account_ids: Some(vec![account.into()]),
                    folder_ids: Some(vec![inbox.into()]),
                    limit: Some(100),
                    cursor: None,
                })
                .await,
        )?;
        if let Some(mail) = page.items.into_iter().find(|mail| mail.subject == subject) {
            return Ok(mail.mail_ref);
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    anyhow::bail!("new self-addressed fixture was not delivered; no send retry performed")
}

async fn reply_forward(
    runtime: &Runtime,
    fixture: &SyntheticMailFixture,
    email: &str,
    attachment: &OutgoingAttachmentInput,
    expected: &[u8],
) -> Result<()> {
    let [first, second] = &fixture.messages;
    let reply_attachment = OutgoingAttachmentInput {
        filename: Some("reply-acceptance.bin".into()),
        ..attachment.clone()
    };
    confirmed(
        runtime
            .mail_reply(MailReplyInput {
                mail_ref: first.mail_ref.clone(),
                body: "EAS Mail MCP automated self-reply attachment acceptance.".into(),
                reply_all: false,
                attachments: vec![reply_attachment],
                idempotency_key: uuid::Uuid::new_v4().to_string(),
            })
            .await,
    )?;
    let reply = wait_for_mail(
        runtime,
        &fixture.account_id,
        &fixture.inbox_id,
        &format!("Re: {}", first.subject),
    )
    .await?;
    verify_attachment(runtime, &reply, "reply-acceptance.bin", expected).await?;
    let forward_attachment = OutgoingAttachmentInput {
        filename: Some("forward-acceptance.bin".into()),
        ..attachment.clone()
    };
    confirmed(
        runtime
            .mail_forward(MailForwardInput {
                mail_ref: second.mail_ref.clone(),
                to: vec![email.into()],
                cc: Vec::new(),
                bcc: Vec::new(),
                body: "EAS Mail MCP automated self-forward attachment acceptance.".into(),
                attachments: vec![forward_attachment],
                idempotency_key: uuid::Uuid::new_v4().to_string(),
            })
            .await,
    )?;
    let forwarded = wait_for_mail(
        runtime,
        &fixture.account_id,
        &fixture.inbox_id,
        &format!("Fwd: {}", second.subject),
    )
    .await?;
    verify_attachment(runtime, &forwarded, "forward-acceptance.bin", expected).await
}

async fn verify_attachment(
    runtime: &Runtime,
    reference: &str,
    filename: &str,
    expected: &[u8],
) -> Result<()> {
    let listed = required(
        runtime.mail_list_attachments(MailAttachmentsInput { mail_ref: reference.into() }).await,
    )?;
    let item = listed
        .attachments
        .iter()
        .find(|item| item.display_name == filename)
        .context("binary attachment is absent")?;
    let downloaded = required(
        runtime
            .mail_download_attachment(AttachmentDownloadInput {
                attachment_ref: item.attachment_ref.clone(),
            })
            .await,
    )?;
    anyhow::ensure!(
        std::fs::read(&downloaded.path)? == expected,
        "attachment bytes changed in transit"
    );
    std::fs::remove_file(downloaded.path)?;
    Ok(())
}

fn required<T>(response: ApiResponse<T>) -> Result<T> {
    if let Some(error) = response.error {
        anyhow::bail!("acceptance failed: {:?}; no automatic write retry", error.code);
    }
    anyhow::ensure!(response.warnings.is_empty(), "acceptance returned partial warnings");
    response.data.context("acceptance returned no result")
}

fn confirmed(response: ApiResponse<OperationResult>) -> Result<()> {
    let result = required(response)?;
    anyhow::ensure!(
        result.status == OperationState::Succeeded,
        "write not confirmed; stop without retry: {}",
        result.operation_id
    );
    Ok(())
}
