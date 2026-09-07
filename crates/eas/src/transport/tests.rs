use std::sync::{Arc, Once};
use std::time::Duration;

use base64::Engine as _;
use rcgen::{CertifiedKey, generate_simple_self_signed};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

use super::{HttpTransport, Transport as _, final_error, retry_delays, strict_client};
use crate::{Command, EasError, RequestSafety};

#[tokio::test]
async fn explicitly_trusted_certificate_is_accepted() -> anyhow::Result<()> {
    let server = TestServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").await?;
    let client = strict_client(Some(server.certificate_pem.as_bytes()))?;
    let response = client.get(server.localhost_url()).send().await?;
    anyhow::ensure!(response.status() == reqwest::StatusCode::OK);
    server.wait().await
}

#[tokio::test]
async fn untrusted_certificate_is_rejected() -> anyhow::Result<()> {
    let server = TestServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").await?;
    let result = strict_client(None)?.get(server.localhost_url()).send().await;
    anyhow::ensure!(result.is_err());
    server.wait_allowing_handshake_failure().await
}

#[tokio::test]
async fn hostname_mismatch_is_rejected() -> anyhow::Result<()> {
    let server = TestServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").await?;
    let client = strict_client(Some(server.certificate_pem.as_bytes()))?;
    let result = client.get(server.loopback_url()).send().await;
    anyhow::ensure!(result.is_err());
    server.wait_allowing_handshake_failure().await
}

#[tokio::test]
async fn redirects_are_returned_without_being_followed() -> anyhow::Result<()> {
    let server = TestServer::start(
        "HTTP/1.1 302 Found\r\nLocation: https://example.com/\r\nContent-Length: 0\r\n\r\n",
    )
    .await?;
    let client = strict_client(Some(server.certificate_pem.as_bytes()))?;
    let response = client.get(server.localhost_url()).send().await?;
    anyhow::ensure!(response.status() == reqwest::StatusCode::FOUND);
    anyhow::ensure!(response.url().host_str() == Some("localhost"));
    server.wait().await
}

#[tokio::test]
async fn http_transport_checks_origin_status_and_request_invariants() -> anyhow::Result<()> {
    let server = TestServer::start("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await?;
    let client = strict_client(Some(server.certificate_pem.as_bytes()))?;
    let transport = HttpTransport::with_test_endpoint(
        client,
        format!("{}Microsoft-Server-ActiveSync", server.localhost_url()),
        "user".into(),
        "fixture-value".into(),
        "00112233445566778899AABBCCDDEEFF".into(),
    )?;
    let response = transport
        .command(Command::Sync, b"fixture-body", Some(7), RequestSafety::RetrySafe)
        .await?;
    anyhow::ensure!(response.status == 200);
    let request = server.request().await?;
    anyhow::ensure!(request.starts_with("POST /Microsoft-Server-ActiveSync?"));
    anyhow::ensure!(
        request.to_ascii_lowercase().contains("content-type: application/vnd.ms-sync.wbxml")
    );
    let user_agent = crate::device::user_agent(env!("CARGO_PKG_VERSION"));
    anyhow::ensure!(request.contains(&user_agent));
    anyhow::ensure!(request.ends_with("fixture-body"));
    let query = request
        .lines()
        .next()
        .and_then(|line| line.split_once('?'))
        .and_then(|(_, suffix)| suffix.split_once(' '))
        .map(|(query, _)| query)
        .ok_or_else(|| anyhow::anyhow!("request query is missing"))?;
    let query = base64::engine::general_purpose::STANDARD.decode(query)?;
    anyhow::ensure!(query.ends_with(b"EasMailMCP"));
    Ok(())
}

#[tokio::test]
async fn http_transport_maps_redirects_auth_and_purges_memory_secrets() -> anyhow::Result<()> {
    for (status, expected) in [(302, "redirect"), (401, "authentication"), (403, "access_denied")] {
        let server =
            TestServer::start(format!("HTTP/1.1 {status} Test\r\nContent-Length: 0\r\n\r\n"))
                .await?;
        let client = strict_client(Some(server.certificate_pem.as_bytes()))?;
        let transport = HttpTransport::with_test_endpoint(
            client,
            format!("{}Microsoft-Server-ActiveSync", server.localhost_url()),
            "user".into(),
            "fixture-value".into(),
            "00112233445566778899AABBCCDDEEFF".into(),
        )?;
        let result = transport.options().await;
        let matched = match expected {
            "authentication" => matches!(result, Err(EasError::Authentication)),
            "access_denied" => matches!(result, Err(EasError::AccessDenied)),
            _ => matches!(result, Err(EasError::Protocol(_))),
        };
        anyhow::ensure!(matched);
        server.wait().await?;
    }

    let server = TestServer::start("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await?;
    let client = strict_client(Some(server.certificate_pem.as_bytes()))?;
    let transport = HttpTransport::with_test_endpoint(
        client,
        format!("{}Microsoft-Server-ActiveSync", server.localhost_url()),
        "user".into(),
        "fixture-value".into(),
        "00112233445566778899AABBCCDDEEFF".into(),
    )?;
    transport.purge_secrets().await;
    transport.options().await?;
    let request = server.request().await?;
    anyhow::ensure!(
        request.contains("authorization: Basic Og==")
            || request.contains("Authorization: Basic Og==")
    );
    Ok(())
}

#[test]
fn retry_policy_is_bounded_and_mutations_fail_as_unknown() {
    assert_eq!(retry_delays(RequestSafety::RetrySafe), [Duration::ZERO, Duration::ZERO]);
    assert!(retry_delays(RequestSafety::Mutation).is_empty());
    let safe = final_error(RequestSafety::RetrySafe, EasError::Network("fixture".into()));
    assert!(matches!(safe, EasError::Network(message) if message == "fixture"));
    let mutation = final_error(RequestSafety::Mutation, EasError::Network("fixture".into()));
    assert!(matches!(mutation, EasError::OutcomeUnknown));
}

struct TestServer {
    port: u16,
    certificate_pem: String,
    task: tokio::task::JoinHandle<anyhow::Result<String>>,
}

#[tokio::test]
async fn response_body_disconnect_after_sent_mutation_is_unknown() -> anyhow::Result<()> {
    for safety in [RequestSafety::Mutation, RequestSafety::RetrySafe] {
        let server = TestServer::start("HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\ncut").await?;
        let transport = test_transport(&server)?;
        let result = transport.command(Command::SendMail, b"sent", Some(7), safety).await;
        match safety {
            RequestSafety::Mutation => {
                anyhow::ensure!(matches!(result, Err(EasError::OutcomeUnknown)))
            }
            RequestSafety::RetrySafe => {
                anyhow::ensure!(matches!(result, Err(EasError::Network(_))))
            }
        }
        anyhow::ensure!(server.request().await?.contains("sent"));
    }
    Ok(())
}

#[tokio::test]
async fn oversized_response_is_rejected_from_headers_before_body_download() -> anyhow::Result<()> {
    let server = TestServer::start("HTTP/1.1 200 OK\r\nContent-Length: 999999999\r\n\r\n").await?;
    let transport = test_transport(&server)?;
    let error =
        transport.command(Command::Search, b"query", Some(7), RequestSafety::RetrySafe).await;
    anyhow::ensure!(matches!(error, Err(EasError::ResponseTooLarge)));
    server.wait().await
}

#[tokio::test]
async fn chunked_response_cannot_exceed_actual_byte_budget() -> anyhow::Result<()> {
    let payload = "x".repeat(65_537);
    let server = TestServer::start(format!(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{payload}\r\n0\r\n\r\n",
        payload.len()
    ))
    .await?;
    let transport = test_transport(&server)?;
    anyhow::ensure!(matches!(transport.options().await, Err(EasError::ResponseTooLarge)));
    let _ = server.wait_allowing_handshake_failure().await;
    Ok(())
}

fn test_transport(server: &TestServer) -> anyhow::Result<HttpTransport> {
    Ok(HttpTransport::with_test_endpoint(
        strict_client(Some(server.certificate_pem.as_bytes()))?,
        format!("{}Microsoft-Server-ActiveSync", server.localhost_url()),
        "user".into(),
        "fixture-value".into(),
        "00112233445566778899AABBCCDDEEFF".into(),
    )?)
}

impl TestServer {
    async fn start(response: impl Into<String>) -> anyhow::Result<Self> {
        install_crypto_provider();
        let response = response.into();
        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["localhost".into()])?;
        let certificate_pem = cert.pem();
        let certificate = cert.der().clone();
        let private_key =
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![certificate], private_key)?;
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await?;
            let mut stream = acceptor.accept(stream).await?;
            let mut request = [0_u8; 4096];
            let count = stream.read(&mut request).await?;
            stream.write_all(response.as_bytes()).await?;
            stream.shutdown().await?;
            let received = request
                .get(..count)
                .ok_or_else(|| anyhow::anyhow!("request length exceeds receive buffer"))?;
            Ok(String::from_utf8_lossy(received).into_owned())
        });
        Ok(Self { port, certificate_pem, task })
    }

    fn localhost_url(&self) -> String {
        format!("https://localhost:{}/", self.port)
    }

    fn loopback_url(&self) -> String {
        format!("https://127.0.0.1:{}/", self.port)
    }

    async fn wait(self) -> anyhow::Result<()> {
        let _ = self.join().await??;
        Ok(())
    }

    async fn request(self) -> anyhow::Result<String> {
        self.join().await?
    }

    async fn wait_allowing_handshake_failure(self) -> anyhow::Result<()> {
        match self.join().await? {
            Ok(_) | Err(_) => Ok(()),
        }
    }

    async fn join(self) -> anyhow::Result<anyhow::Result<String>> {
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .map_err(|_| anyhow::anyhow!("local TLS server timed out"))?
            .map_err(Into::into)
    }
}

fn install_crypto_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
    });
}
