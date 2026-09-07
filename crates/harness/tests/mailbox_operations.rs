#[expect(dead_code, reason = "shared integration-test support is compiled once per test binary")]
mod support;

use eas_mail_mcp::backend::{AccountBackend as _, MailSource};
use eas_mail_mcp::{ErrorCode, SecretStore as _};
use eas_mail_protocol::protocol::{
    ComposeSource, build_attachment_fetch, build_initial_provision, build_item_fetch,
    build_mime_message, build_policy_ack, build_search, build_send, build_smart, build_wipe_ack,
};
use eas_mail_protocol::{Command, RequestSafety};

use support::{
    attachment_response, call, compose_response, default_policy, item_response, mailbox,
    mailbox_with_store, mutation, options, outgoing, policy, provision_response, read,
    search_response, wipe_response,
};

#[tokio::test]
async fn search_refreshes_policy_and_fetches_full_item() -> anyhow::Result<()> {
    let initial_policy = provision_response(1, Some(700), None)?;
    let final_policy = provision_response(1, Some(701), Some(1))?;
    let calls = vec![
        options(),
        call(
            Command::Search,
            build_search("report", 0, 10, 500)?,
            Some(123),
            RequestSafety::RetrySafe,
            449,
            Vec::new(),
        ),
        call(
            Command::Provision,
            build_initial_provision()?,
            None,
            RequestSafety::RetrySafe,
            200,
            initial_policy,
        ),
        call(
            Command::Provision,
            build_policy_ack(700, true)?,
            Some(0),
            RequestSafety::RetrySafe,
            200,
            final_policy,
        ),
        call(
            Command::Search,
            build_search("report", 0, 10, 500)?,
            Some(701),
            RequestSafety::RetrySafe,
            200,
            search_response()?,
        ),
        call(
            Command::ItemOperations,
            build_item_fetch(Some("long-1"), None, None, 12_000)?,
            Some(701),
            RequestSafety::RetrySafe,
            200,
            item_response()?,
        ),
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    let found = mailbox.search_mail("report", 10).await?;
    assert_eq!(found.len(), 1);
    let source = found
        .first()
        .map(|mail| mail.source.clone())
        .ok_or_else(|| anyhow::anyhow!("search result is missing"))?;
    let full = mailbox.fetch_mail(&source, 12_000).await?;
    assert_eq!(full.folder_id, "");
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn attachment_policy_blocks_disabled_and_oversized_payloads() -> anyhow::Result<()> {
    let (disabled, disabled_transport) = mailbox(vec![options()], policy(10, false))?;
    let error = disabled.fetch_attachment("file").await.map_err(|error| error.envelope.code);
    assert_eq!(error, Err(ErrorCode::PolicyBlocked));
    disabled_transport.verify_complete()?;

    let calls = vec![
        options(),
        read(
            Command::ItemOperations,
            build_attachment_fetch("file")?,
            attachment_response(b"too large")?,
        ),
    ];
    let (limited, transport) = mailbox(calls, policy(4, true))?;
    let error = limited.fetch_attachment("file").await.map_err(|error| error.envelope.code);
    assert_eq!(error, Err(ErrorCode::PolicyBlocked));
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn send_reply_and_forward_use_mutation_requests() -> anyhow::Result<()> {
    let message = outgoing();
    let mime = build_mime_message(
        "user@example.invalid",
        &message.to,
        &message.cc,
        &message.bcc,
        &message.subject,
        &message.body,
    )?;
    let source = MailSource::Item { folder_id: "inbox".into(), server_id: "message-1".into() };
    let calls = vec![
        options(),
        read(
            Command::ItemOperations,
            build_item_fetch(Some("long-1"), None, None, 1)?,
            item_response()?,
        ),
        mutation(Command::SendMail, build_send("send-id", mime.clone())?, Vec::new()),
        mutation(
            Command::SmartReply,
            build_smart(
                false,
                "reply-id",
                ComposeSource::Item { folder_id: "inbox", item_id: "message-1" },
                mime.clone(),
            )?,
            Vec::new(),
        ),
        mutation(
            Command::SmartForward,
            build_smart(true, "forward-id", ComposeSource::LongId("long-1"), mime)?,
            Vec::new(),
        ),
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    assert_eq!(
        mailbox
            .mark_read(&MailSource::LongId("long-1".into()), true)
            .await
            .map_err(|error| error.envelope.code),
        Err(ErrorCode::FeatureUnavailable)
    );
    mailbox.send("send-id", &message).await?;
    mailbox.reply("reply-id", &source, &message).await?;
    mailbox.forward("forward-id", &MailSource::LongId("long-1".into()), &message).await?;
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn rejected_mutation_returns_protocol_error() -> anyhow::Result<()> {
    let message = outgoing();
    let mime = build_mime_message(
        "user@example.invalid",
        &message.to,
        &message.cc,
        &message.bcc,
        &message.subject,
        &message.body,
    )?;
    let calls = vec![
        options(),
        mutation(Command::SendMail, build_send("send-id", mime)?, compose_response(122)?),
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    assert_eq!(
        mailbox.send("send-id", &message).await.map_err(|error| error.envelope.code),
        Err(ErrorCode::ProtocolError)
    );
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn definitive_449_refreshes_policy_before_retrying_a_write() -> anyhow::Result<()> {
    let message = outgoing();
    let mime = build_mime_message(
        "user@example.invalid",
        &message.to,
        &message.cc,
        &message.bcc,
        &message.subject,
        &message.body,
    )?;
    let calls = vec![
        options(),
        call(
            Command::SendMail,
            build_send("send-id", mime.clone())?,
            Some(123),
            RequestSafety::Mutation,
            449,
            Vec::new(),
        ),
        call(
            Command::Provision,
            build_initial_provision()?,
            None,
            RequestSafety::RetrySafe,
            200,
            provision_response(1, Some(700), None)?,
        ),
        call(
            Command::Provision,
            build_policy_ack(700, true)?,
            Some(0),
            RequestSafety::RetrySafe,
            200,
            provision_response(1, Some(701), Some(1))?,
        ),
        call(
            Command::SendMail,
            build_send("send-id", mime)?,
            Some(701),
            RequestSafety::Mutation,
            200,
            Vec::new(),
        ),
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    mailbox.send("send-id", &message).await?;
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn remote_wipe_during_policy_refresh_is_account_scoped() -> anyhow::Result<()> {
    let calls = vec![
        options(),
        call(
            Command::Search,
            build_search("report", 0, 10, 500)?,
            Some(123),
            RequestSafety::RetrySafe,
            449,
            Vec::new(),
        ),
        call(
            Command::Provision,
            build_initial_provision()?,
            None,
            RequestSafety::RetrySafe,
            200,
            wipe_response()?,
        ),
        call(
            Command::Provision,
            build_wipe_ack(true)?,
            Some(77),
            RequestSafety::Mutation,
            204,
            Vec::new(),
        ),
    ];
    let (mailbox, transport, secrets) = mailbox_with_store(calls, default_policy())?;
    assert_eq!(
        mailbox.search_mail("report", 10).await.map_err(|error| error.envelope.code),
        Err(ErrorCode::RemoteWipe)
    );
    assert!(!secrets.load()?.accounts.contains_key("work"));
    transport.verify_complete()?;
    Ok(())
}
