use async_trait::async_trait;
use reqwest::redirect::Policy;
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::sync::Mutex;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::device;
use crate::{Command, EasError, Profile, Result, build_binary_query};

#[cfg(not(test))]
const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(2)];
#[cfg(test)]
const RETRY_DELAYS: [Duration; 2] = [Duration::ZERO, Duration::ZERO];

/// Whether a failed EAS request can be retried without duplicating a mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestSafety {
    /// Read-only or idempotent request; retry transient network failures.
    RetrySafe,
    /// External mutation; never retry after an ambiguous send.
    Mutation,
}

/// Transport-neutral EAS response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportResponse {
    /// HTTP status code.
    pub status: u16,
    /// Raw WBXML response body.
    pub body: Vec<u8>,
    /// Lowercase response headers needed by EAS negotiation.
    pub headers: BTreeMap<String, String>,
}

/// I/O boundary used by the real HTTP client and deterministic harness.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Performs EAS OPTIONS negotiation.
    async fn options(&self) -> Result<TransportResponse>;

    /// Sends one ActiveSync command.
    async fn command(
        &self,
        command: Command,
        body: &[u8],
        policy_key: Option<u32>,
        safety: RequestSafety,
    ) -> Result<TransportResponse>;

    /// Irreversibly clears process-local credentials after an EAS remote wipe.
    async fn purge_secrets(&self);
}

#[derive(Zeroize, ZeroizeOnDrop)]
struct Credentials {
    username: String,
    password: String,
    device_id: String,
}

/// Strict HTTPS transport for one fixed managed profile.
pub struct HttpTransport {
    client: reqwest::Client,
    profile: Profile,
    credentials: Mutex<Credentials>,
    #[cfg(test)]
    endpoint_override: Option<String>,
}

impl HttpTransport {
    /// Constructs a transport with mandatory TLS and disabled redirects.
    pub fn new(
        profile: &Profile,
        username: String,
        password: String,
        device_id: String,
    ) -> Result<Self> {
        let client = strict_client(profile.extra_ca_pem.as_deref())?;
        profile.validate_device_id(&device_id)?;
        build_binary_query(Command::Sync, &device_id, 0, false)?;
        Ok(Self {
            client,
            profile: profile.clone(),
            credentials: Mutex::new(Credentials { username, password, device_id }),
            #[cfg(test)]
            endpoint_override: None,
        })
    }

    async fn send_once(
        &self,
        command: Command,
        body: &[u8],
        policy_key: Option<u32>,
    ) -> Result<reqwest::Response> {
        let credentials = self.credentials.lock().await;
        let query = build_binary_query(
            command,
            &credentials.device_id,
            policy_key.unwrap_or(0),
            policy_key.is_none(),
        )?;
        let request = self
            .client
            .post(format!("{}?{query}", self.endpoint()))
            .basic_auth(&credentials.username, Some(&credentials.password))
            .header("Content-Type", "application/vnd.ms-sync.wbxml")
            .body(body.to_vec());
        drop(credentials);
        request.send().await.map_err(|error| EasError::Network(error.to_string()))
    }

    async fn normalize(&self, response: reqwest::Response) -> Result<TransportResponse> {
        let url = response.url();
        if url.scheme() != "https" || url.host_str() != Some(self.profile.host.as_str()) {
            return Err(EasError::Protocol("Exchange response origin changed".into()));
        }
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            return Err(EasError::Protocol("Exchange attempted an HTTP redirect".into()));
        }
        if status == 401 || status == 403 {
            return Err(EasError::Authentication);
        }
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value.to_str().ok().map(|value| (name.as_str().to_owned(), value.to_owned()))
            })
            .collect();
        let body =
            response.bytes().await.map_err(|error| EasError::Network(error.to_string()))?.to_vec();
        Ok(TransportResponse { status, body, headers })
    }

    fn endpoint(&self) -> String {
        #[cfg(test)]
        if let Some(endpoint) = &self.endpoint_override {
            return endpoint.clone();
        }
        self.profile.endpoint()
    }

    #[cfg(test)]
    fn with_test_endpoint(
        client: reqwest::Client,
        endpoint: String,
        username: String,
        password: String,
        device_id: String,
    ) -> Result<Self> {
        build_binary_query(Command::Sync, &device_id, 0, false)?;
        Ok(Self {
            client,
            profile: Profile::localhost(),
            credentials: Mutex::new(Credentials { username, password, device_id }),
            endpoint_override: Some(endpoint),
        })
    }
}

fn strict_client(extra_ca_pem: Option<&[u8]>) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .redirect(Policy::none())
        .http1_only()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .user_agent(device::user_agent(env!("CARGO_PKG_VERSION")));
    if let Some(pem) = extra_ca_pem {
        let certificate = reqwest::Certificate::from_pem(pem)
            .map_err(|error| EasError::InvalidConfiguration(error.to_string()))?;
        builder = builder.tls_built_in_root_certs(false).add_root_certificate(certificate);
    }
    builder.build().map_err(|error| EasError::InvalidConfiguration(error.to_string()))
}

#[async_trait]
impl Transport for HttpTransport {
    async fn options(&self) -> Result<TransportResponse> {
        let credentials = self.credentials.lock().await;
        let request = self
            .client
            .request(reqwest::Method::OPTIONS, self.endpoint())
            .basic_auth(&credentials.username, Some(&credentials.password));
        drop(credentials);
        let response =
            request.send().await.map_err(|error| EasError::Network(error.to_string()))?;
        self.normalize(response).await
    }

    async fn command(
        &self,
        command: Command,
        body: &[u8],
        policy_key: Option<u32>,
        safety: RequestSafety,
    ) -> Result<TransportResponse> {
        for delay in retry_delays(safety) {
            match self.send_once(command, body, policy_key).await {
                Ok(response) => return self.normalize(response).await,
                Err(_) => tokio::time::sleep(*delay).await,
            }
        }
        match self.send_once(command, body, policy_key).await {
            Ok(response) => self.normalize(response).await,
            Err(error) => Err(final_error(safety, error)),
        }
    }

    async fn purge_secrets(&self) {
        self.credentials.lock().await.zeroize();
    }
}

fn retry_delays(safety: RequestSafety) -> &'static [Duration] {
    match safety {
        RequestSafety::RetrySafe => &RETRY_DELAYS,
        RequestSafety::Mutation => &[],
    }
}

fn final_error(safety: RequestSafety, error: EasError) -> EasError {
    match safety {
        RequestSafety::RetrySafe => error,
        RequestSafety::Mutation => EasError::OutcomeUnknown,
    }
}

#[cfg(test)]
mod tests;
