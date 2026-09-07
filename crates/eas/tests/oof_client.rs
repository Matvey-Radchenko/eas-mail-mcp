use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{
    Command, EasClient, EasError, OofSettings, OofState, RequestSafety, Transport,
    TransportResponse,
};

#[tokio::test]
async fn oof_client_uses_retry_safe_get_and_single_mutation_set() -> anyhow::Result<()> {
    let mut get = Element::new("Settings", "Get");
    get.push(Element::text("Settings", "OofState", "0"));
    let transport =
        Arc::new(OofTransport::new(vec![Ok(response(Some(get), "1")?), Ok(response(None, "1")?)]));
    let client = EasClient::new(transport.clone());
    assert!(client.options().await?.supports(Command::Settings));
    let settings = client.get_oof(7).await?;
    assert_eq!(settings.state, OofState::Disabled);
    client.set_oof(7, &settings).await?;
    let calls = transport.calls.lock().map_err(|_| anyhow::anyhow!("call lock"))?;
    assert_eq!(*calls, vec![RequestSafety::RetrySafe, RequestSafety::Mutation]);
    Ok(())
}

#[tokio::test]
async fn malformed_acknowledgement_and_lost_response_are_unknown_without_retry()
-> anyhow::Result<()> {
    for response in [
        Ok(TransportResponse { status: 200, headers: BTreeMap::new(), body: vec![0xff] }),
        Err(EasError::OutcomeUnknown),
    ] {
        let transport = Arc::new(OofTransport::new(vec![response]));
        let client = EasClient::new(transport.clone());
        assert!(matches!(client.set_oof(7, &disabled()).await, Err(EasError::OutcomeUnknown)));
        assert_eq!(transport.calls.lock().map_err(|_| anyhow::anyhow!("call lock"))?.len(), 1);
    }
    let client = EasClient::new(Arc::new(OofTransport::new(vec![Ok(response(None, "2")?)])));
    assert!(matches!(client.set_oof(7, &disabled()).await, Err(EasError::Protocol(_))));
    Ok(())
}

fn disabled() -> OofSettings {
    OofSettings { state: OofState::Disabled, starts_at: None, ends_at: None, messages: Vec::new() }
}

#[tokio::test]
async fn undefined_settings_status_after_set_is_unknown_and_sends_only_once() -> anyhow::Result<()>
{
    for (outer, inner) in [("0", "1"), ("999", "1"), ("1", "0"), ("1", "999"), ("1", "3")] {
        let transport =
            Arc::new(OofTransport::new(vec![Ok(response_statuses(None, outer, inner)?)]));
        let client = EasClient::new(transport.clone());
        assert!(matches!(client.set_oof(7, &disabled()).await, Err(EasError::OutcomeUnknown)));
        assert_eq!(
            *transport.calls.lock().map_err(|_| anyhow::anyhow!("call lock"))?,
            [RequestSafety::Mutation]
        );
    }
    Ok(())
}

#[tokio::test]
async fn server_processing_failure_does_not_claim_safe_rejection_or_retry() -> anyhow::Result<()> {
    for (outer, inner) in [("110", "1"), ("111", "1"), ("1", "110"), ("1", "111")] {
        let transport =
            Arc::new(OofTransport::new(vec![Ok(response_statuses(None, outer, inner)?)]));
        let client = EasClient::new(transport.clone());
        assert!(matches!(client.set_oof(7, &disabled()).await, Err(EasError::OutcomeUnknown)));
        assert_eq!(
            *transport.calls.lock().map_err(|_| anyhow::anyhow!("call lock"))?,
            [RequestSafety::Mutation]
        );
    }
    Ok(())
}

fn response(get: Option<Element>, status: &str) -> anyhow::Result<TransportResponse> {
    response_statuses(get, "1", status)
}

fn response_statuses(
    get: Option<Element>,
    outer: &str,
    status: &str,
) -> anyhow::Result<TransportResponse> {
    let mut root = Element::new("Settings", "Settings");
    root.push(Element::text("Settings", "Status", outer));
    let mut oof = Element::new("Settings", "Oof");
    oof.push(Element::text("Settings", "Status", status));
    if let Some(get) = get {
        oof.push(get);
    }
    root.push(oof);
    Ok(TransportResponse { status: 200, headers: BTreeMap::new(), body: encode(&root)? })
}

struct OofTransport {
    responses: Mutex<VecDeque<eas_mail_protocol::Result<TransportResponse>>>,
    calls: Mutex<Vec<RequestSafety>>,
}

impl OofTransport {
    fn new(responses: Vec<eas_mail_protocol::Result<TransportResponse>>) -> Self {
        Self { responses: Mutex::new(responses.into()), calls: Mutex::new(Vec::new()) }
    }
}

#[async_trait]
impl Transport for OofTransport {
    async fn options(&self) -> eas_mail_protocol::Result<TransportResponse> {
        Ok(TransportResponse {
            status: 200,
            body: Vec::new(),
            headers: BTreeMap::from([
                ("ms-asprotocolversions".into(), "14.1".into()),
                (
                    "ms-asprotocolcommands".into(),
                    "Provision,FolderSync,Sync,Search,ItemOperations,Settings".into(),
                ),
            ]),
        })
    }
    async fn command(
        &self,
        command: Command,
        _: &[u8],
        key: Option<u32>,
        safety: RequestSafety,
    ) -> eas_mail_protocol::Result<TransportResponse> {
        assert_eq!(command, Command::Settings);
        assert_eq!(key, Some(7));
        self.calls.lock().map_err(|_| EasError::Protocol("call lock".into()))?.push(safety);
        self.responses
            .lock()
            .map_err(|_| EasError::Protocol("response lock".into()))?
            .pop_front()
            .ok_or_else(|| EasError::Protocol("unexpected extra request".into()))?
    }
    async fn purge_secrets(&self) {}
}
