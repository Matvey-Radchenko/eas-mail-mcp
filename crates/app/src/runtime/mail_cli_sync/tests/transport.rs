use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use async_trait::async_trait;
use eas_mail_protocol::{Command, EasError, RequestSafety, Transport, TransportResponse};

pub(super) struct Expected {
    pub(super) command: Command,
    pub(super) body: Vec<u8>,
    pub(super) response: Vec<u8>,
    pub(super) safety: RequestSafety,
}

pub(super) struct Script {
    calls: Mutex<VecDeque<Expected>>,
    options_pending: Mutex<bool>,
}

impl Script {
    pub(super) fn new(calls: Vec<Expected>) -> Self {
        Self { options_pending: Mutex::new(!calls.is_empty()), calls: Mutex::new(calls.into()) }
    }

    pub(super) fn verify_complete(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!*self.options_pending.lock().map_err(|_| protocol())?);
        anyhow::ensure!(self.calls.lock().map_err(|_| protocol())?.is_empty());
        Ok(())
    }
}

#[async_trait]
impl Transport for Script {
    async fn options(&self) -> eas_mail_protocol::Result<TransportResponse> {
        let mut pending = self.options_pending.lock().map_err(|_| protocol())?;
        if !*pending {
            return Err(protocol());
        }
        *pending = false;
        Ok(TransportResponse {
            status: 200,
            body: Vec::new(),
            headers: BTreeMap::from([
                ("ms-asprotocolversions".into(), "14.1".into()),
                (
                    "ms-asprotocolcommands".into(),
                    "Provision,FolderSync,Sync,Search,ItemOperations".into(),
                ),
            ]),
        })
    }

    async fn command(
        &self,
        command: Command,
        body: &[u8],
        key: Option<u32>,
        safety: RequestSafety,
    ) -> eas_mail_protocol::Result<TransportResponse> {
        let expected =
            self.calls.lock().map_err(|_| protocol())?.pop_front().ok_or_else(protocol)?;
        if expected.command != command
            || expected.body != body
            || key != Some(123)
            || safety != expected.safety
        {
            return Err(protocol());
        }
        Ok(TransportResponse { status: 200, body: expected.response, headers: BTreeMap::new() })
    }

    async fn purge_secrets(&self) {}
}

fn protocol() -> EasError {
    EasError::Protocol("unexpected CLI synchronization test request".into())
}
