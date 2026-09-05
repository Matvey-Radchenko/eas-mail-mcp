use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{
    Command, EasClient, EasError, RequestSafety, Result, Transport, TransportResponse,
};

pub struct Boundary {
    responses: Mutex<VecDeque<Result<TransportResponse>>>,
    calls: Mutex<Vec<Call>>,
}

struct Call {
    command: Command,
    body: Vec<u8>,
    key: Option<u32>,
    safety: RequestSafety,
}

impl Boundary {
    pub fn client(response: Result<TransportResponse>) -> (EasClient, Arc<Self>) {
        let boundary = Arc::new(Self {
            responses: Mutex::new(VecDeque::from([response])),
            calls: Mutex::new(Vec::new()),
        });
        (EasClient::new(boundary.clone()), boundary)
    }

    pub fn request(&self, command: Command) -> anyhow::Result<Element> {
        let calls = self.calls.lock().map_err(|_| anyhow::anyhow!("call lock"))?;
        assert_eq!(calls.len(), 1, "a mutation must not be sent twice");
        let call = calls.first().ok_or_else(|| anyhow::anyhow!("no call"))?;
        assert_eq!(call.command, command);
        assert_eq!(call.key, Some(7));
        assert_eq!(call.safety, RequestSafety::Mutation);
        eas_mail_protocol::wbxml::decode(&call.body)?
            .ok_or_else(|| anyhow::anyhow!("empty request"))
    }

    pub fn assert_no_calls(&self) -> anyhow::Result<()> {
        assert!(self.calls.lock().map_err(|_| anyhow::anyhow!("call lock"))?.is_empty());
        Ok(())
    }
}

#[async_trait]
impl Transport for Boundary {
    async fn options(&self) -> Result<TransportResponse> {
        Err(EasError::Protocol("unexpected OPTIONS during mutation".into()))
    }
    async fn command(
        &self,
        command: Command,
        body: &[u8],
        key: Option<u32>,
        safety: RequestSafety,
    ) -> Result<TransportResponse> {
        self.calls.lock().map_err(|_| EasError::Protocol("call lock".into()))?.push(Call {
            command,
            body: body.to_vec(),
            key,
            safety,
        });
        self.responses
            .lock()
            .map_err(|_| EasError::Protocol("response lock".into()))?
            .pop_front()
            .ok_or_else(|| EasError::Protocol("unexpected retry".into()))?
    }
    async fn purge_secrets(&self) {}
}

pub fn http(status: u16, body: Vec<u8>, retry_after: Option<&str>) -> TransportResponse {
    let headers = retry_after
        .map(|value| BTreeMap::from([("retry-after".into(), value.into())]))
        .unwrap_or_default();
    TransportResponse { status, body, headers }
}

pub fn collection(fields: &[(&str, &str)], changes: Option<Vec<Element>>) -> Element {
    let mut collection = Element::new("AirSync", "Collection");
    for (name, value) in fields {
        collection.push(Element::text("AirSync", *name, *value));
    }
    if let Some(changes) = changes {
        let mut responses = Element::new("AirSync", "Responses");
        for change in changes {
            responses.push(change);
        }
        collection.push(responses);
    }
    collection
}

pub fn change(fields: &[(&str, &str)]) -> Element {
    let mut change = Element::new("AirSync", "Change");
    for (name, value) in fields {
        change.push(Element::text("AirSync", *name, *value));
    }
    change
}

pub fn sync(collection: Option<Element>) -> anyhow::Result<Vec<u8>> {
    let mut root = Element::new("AirSync", "Sync");
    let mut collections = Element::new("AirSync", "Collections");
    if let Some(collection) = collection {
        collections.push(collection);
    }
    root.push(collections);
    Ok(encode(&root)?)
}

pub fn accepted(changes: Option<Vec<Element>>) -> anyhow::Result<Vec<u8>> {
    sync(Some(collection(
        &[("CollectionId", "inbox"), ("Status", "1"), ("SyncKey", "next")],
        changes,
    )))
}

pub fn moved(fields: &[(&str, &str)]) -> anyhow::Result<Vec<u8>> {
    let mut root = Element::new("Move", "MoveItems");
    let mut response = Element::new("Move", "Response");
    for (name, value) in fields {
        response.push(Element::text("Move", *name, *value));
    }
    root.push(response);
    Ok(encode(&root)?)
}

pub fn text(tree: &Element, namespace: &str, name: &str) -> Option<String> {
    tree.descendant(namespace, name).map(Element::text_content)
}
