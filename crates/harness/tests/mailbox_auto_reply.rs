#[expect(dead_code, reason = "shared integration-test support is compiled once per test binary")]
mod support;

use eas_mail_mcp::backend::AccountBackend as _;
use eas_mail_mcp::{ErrorCode, SecretStore as _};
use eas_mail_mcp_harness::{ExpectedCall, ScriptedFailure};
use eas_mail_protocol::protocol::{
    build_initial_provision, build_oof_get, build_oof_set, build_policy_ack,
};
use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{Command, OofSettings, OofState, RequestSafety};

#[tokio::test]
async fn absent_settings_capability_blocks_get_and_set_without_a_command() -> anyhow::Result<()> {
    let (mailbox, transport, secrets) =
        support::mailbox_with_store(vec![support::options()], support::default_policy())?;
    let before = serde_json::to_value(secrets.load()?)?;
    assert_eq!(mailbox.get_auto_reply().await.map_err(code), Err(ErrorCode::FeatureUnavailable));
    assert_eq!(
        mailbox.set_auto_reply(&disabled()).await.map_err(code),
        Err(ErrorCode::FeatureUnavailable)
    );
    transport.verify_complete()?;
    assert_eq!(serde_json::to_value(secrets.load()?)?, before);
    Ok(())
}

#[tokio::test]
async fn get_and_set_use_exact_settings_requests_and_preserve_credentials() -> anyhow::Result<()> {
    let (mailbox, transport, secrets) = support::mailbox_with_store(
        vec![
            options(),
            support::read(Command::Settings, build_oof_get()?, response(true, "1")?),
            support::mutation(
                Command::Settings,
                build_oof_set(&disabled())?,
                response(false, "1")?,
            ),
            support::read(Command::Settings, build_oof_get()?, response(true, "1")?),
        ],
        support::default_policy(),
    )?;
    let before = serde_json::to_value(secrets.load()?)?;
    assert_eq!(mailbox.get_auto_reply().await?, disabled());
    mailbox.set_auto_reply(&disabled()).await?;
    assert_eq!(mailbox.get_auto_reply().await?, disabled());
    transport.verify_complete()?;
    assert_eq!(serde_json::to_value(secrets.load()?)?, before);
    Ok(())
}

#[tokio::test]
async fn rejected_policy_refresh_retries_get_or_set_only_after_persisting_new_policy()
-> anyhow::Result<()> {
    for mutation in [false, true] {
        let body = if mutation { build_oof_set(&disabled())? } else { build_oof_get()? };
        let safety = if mutation { RequestSafety::Mutation } else { RequestSafety::RetrySafe };
        let (mailbox, transport, secrets) = support::mailbox_with_store(
            vec![
                options(),
                support::call(Command::Settings, body.clone(), Some(123), safety, 449, Vec::new()),
                support::call(
                    Command::Provision,
                    build_initial_provision()?,
                    None,
                    RequestSafety::RetrySafe,
                    200,
                    support::provision_response(1, Some(700), None)?,
                ),
                support::call(
                    Command::Provision,
                    build_policy_ack(700, true)?,
                    Some(0),
                    RequestSafety::RetrySafe,
                    200,
                    support::provision_response(1, Some(701), Some(1))?,
                ),
                support::call(
                    Command::Settings,
                    body,
                    Some(701),
                    safety,
                    200,
                    response(!mutation, "1")?,
                ),
            ],
            support::default_policy(),
        )?;
        let before = secrets.load()?;
        if mutation {
            mailbox.set_auto_reply(&disabled()).await?;
        } else {
            mailbox.get_auto_reply().await?;
        }
        let after = secrets.load()?;
        assert!(before.hmac_key == after.hmac_key);
        let secret =
            after.accounts.get("work").ok_or_else(|| anyhow::anyhow!("account disappeared"))?;
        assert_eq!(secret.policy_key, 701);
        assert_eq!(secret.device_id, "0011223344556677");
        transport.verify_complete()?;
    }
    Ok(())
}

#[tokio::test]
async fn lost_or_rejected_set_never_retries_and_preserves_scoped_error() -> anyhow::Result<()> {
    for (failure, response, expected) in [
        (Some(ScriptedFailure::OutcomeUnknown), Vec::new(), ErrorCode::OutcomeUnknown),
        (None, vec![0xff], ErrorCode::OutcomeUnknown),
        (None, response(false, "2")?, ErrorCode::ProtocolError),
    ] {
        let command = ExpectedCall::Command {
            command: Command::Settings,
            body: build_oof_set(&disabled())?,
            policy_key: Some(123),
            safety: RequestSafety::Mutation,
            status: 200,
            response,
            delay: std::time::Duration::ZERO,
            failure,
        };
        let (mailbox, transport, secrets) =
            support::mailbox_with_store(vec![options(), command], support::default_policy())?;
        let before = serde_json::to_value(secrets.load()?)?;
        let error = mailbox
            .set_auto_reply(&disabled())
            .await
            .err()
            .ok_or_else(|| anyhow::anyhow!("unsafe Set succeeded"))?;
        assert_eq!(error.envelope.code, expected);
        assert_eq!(error.envelope.account_id.as_deref(), Some("work"));
        transport.verify_complete()?;
        assert_eq!(serde_json::to_value(secrets.load()?)?, before);
    }
    Ok(())
}

fn options() -> ExpectedCall {
    let mut call = support::options();
    if let ExpectedCall::Options { headers, .. } = &mut call {
        headers.insert(
            "ms-asprotocolcommands".into(),
            "Provision,FolderSync,Sync,Search,ItemOperations,Settings".into(),
        );
    }
    call
}

fn disabled() -> OofSettings {
    OofSettings { state: OofState::Disabled, starts_at: None, ends_at: None, messages: Vec::new() }
}

fn response(get: bool, status: &str) -> anyhow::Result<Vec<u8>> {
    let mut root = Element::new("Settings", "Settings");
    root.push(Element::text("Settings", "Status", "1"));
    let mut oof = Element::new("Settings", "Oof");
    oof.push(Element::text("Settings", "Status", status));
    if get {
        let mut get = Element::new("Settings", "Get");
        get.push(Element::text("Settings", "OofState", "0"));
        oof.push(get);
    }
    root.push(oof);
    Ok(encode(&root)?)
}

fn code(error: eas_mail_mcp::AppError) -> ErrorCode {
    error.envelope.code
}
