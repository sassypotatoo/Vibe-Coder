use crate::config::{OmniRouteConfig, is_loopback_host};
use reqwest::{Client, Method, StatusCode};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
use url::Url;
use vibecoder_domain::{Result, VibeCoderError};

pub const VIBECODER_FREE_PROVIDER_ID: &str = "opencode";
pub const VIBECODER_FREE_PROVIDER_NAME: &str = "OpenCode Free";

const MAX_MANAGEMENT_RESPONSE_BYTES: usize = 512 * 1024;
const USER_AGENT: &str = concat!("vibecoder/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreeProviderBootstrapOutcome {
    AlreadyConfigured,
    Reactivated,
    Created,
}

#[derive(Clone)]
pub struct OmniRouteProviderBootstrap {
    http: Client,
    api_root: Url,
}

impl std::fmt::Debug for OmniRouteProviderBootstrap {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OmniRouteProviderBootstrap")
            .field("api_root", &self.api_root.as_str())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Deserialize)]
struct ProviderListEnvelope {
    #[serde(default)]
    connections: Vec<ProviderConnection>,
}

#[derive(Debug, Deserialize)]
struct ProviderConnectionEnvelope {
    connection: ProviderConnection,
}

#[derive(Debug, Deserialize)]
struct ProviderConnection {
    id: String,
    provider: String,
    #[serde(default, rename = "isActive")]
    is_active: bool,
}

impl OmniRouteProviderBootstrap {
    pub fn new(config: OmniRouteConfig) -> Result<Self> {
        let validated = config.validated()?;
        if !is_loopback_host(validated.api_base.host()) {
            return Err(gateway_error("provider_bootstrap_non_loopback_forbidden"));
        }
        let api_root = validated
            .api_base
            .join("../api/")
            .map_err(|_| gateway_error("provider_bootstrap_url_invalid"))?;
        if api_root.path() != "/api/" {
            return Err(gateway_error("provider_bootstrap_url_invalid"));
        }
        let http = Client::builder()
            .connect_timeout(Duration::from_millis(validated.request_timeout_ms))
            .timeout(Duration::from_millis(validated.request_timeout_ms))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .user_agent(USER_AGENT)
            .build()
            .map_err(|_| gateway_error("provider_bootstrap_http_client_failed"))?;
        Ok(Self { http, api_root })
    }

    pub async fn ensure_vibecoder_free_provider(&self) -> Result<FreeProviderBootstrapOutcome> {
        let mut provider_url = self.management_url("providers")?;
        provider_url
            .query_pairs_mut()
            .append_pair("provider", VIBECODER_FREE_PROVIDER_ID)
            .append_pair("limit", "10");
        let listed = self.send(Method::GET, provider_url, None).await?;
        expect_success(listed.status, "provider_bootstrap_list")?;
        let envelope: ProviderListEnvelope = serde_json::from_slice(&listed.body)
            .map_err(|_| gateway_error("provider_bootstrap_list_invalid_json"))?;

        let mut matching = envelope
            .connections
            .into_iter()
            .filter(|connection| connection.provider == VIBECODER_FREE_PROVIDER_ID);
        if let Some(connection) = matching.next() {
            validate_connection_id(&connection.id)?;
            if connection.is_active {
                self.sync_models(&connection.id).await?;
                return Ok(FreeProviderBootstrapOutcome::AlreadyConfigured);
            }
            self.reactivate_provider(&connection.id).await?;
            self.sync_models(&connection.id).await?;
            return Ok(FreeProviderBootstrapOutcome::Reactivated);
        }

        let body = serde_json::to_vec(&json!({
            "provider": VIBECODER_FREE_PROVIDER_ID,
            "name": VIBECODER_FREE_PROVIDER_NAME,
            "priority": 1
        }))
        .map_err(|_| gateway_error("provider_bootstrap_request_encode_failed"))?;
        let created = self
            .send(Method::POST, self.management_url("providers")?, Some(body))
            .await?;
        if created.status != StatusCode::CREATED.as_u16() && !(200..300).contains(&created.status) {
            return Err(status_error("provider_bootstrap_create", created.status));
        }
        let envelope: ProviderConnectionEnvelope = serde_json::from_slice(&created.body)
            .map_err(|_| gateway_error("provider_bootstrap_create_invalid_json"))?;
        if envelope.connection.provider != VIBECODER_FREE_PROVIDER_ID {
            return Err(gateway_error("provider_bootstrap_create_provider_mismatch"));
        }
        validate_connection_id(&envelope.connection.id)?;
        self.sync_models(&envelope.connection.id).await?;
        Ok(FreeProviderBootstrapOutcome::Created)
    }

    async fn reactivate_provider(&self, id: &str) -> Result<()> {
        let body = serde_json::to_vec(&json!({ "isActive": true }))
            .map_err(|_| gateway_error("provider_bootstrap_request_encode_failed"))?;
        let path = format!("providers/{id}");
        let response = self
            .send(Method::PATCH, self.management_url(&path)?, Some(body))
            .await?;
        expect_success(response.status, "provider_bootstrap_reactivate")
    }

    async fn sync_models(&self, id: &str) -> Result<()> {
        let path = format!("providers/{id}/sync-models");
        let mut url = self.management_url(&path)?;
        url.query_pairs_mut().append_pair("mode", "import");
        let response = self.send(Method::POST, url, Some(b"{}".to_vec())).await?;
        expect_success(response.status, "provider_bootstrap_model_sync")
    }

    fn management_url(&self, relative: &str) -> Result<Url> {
        if relative.is_empty()
            || relative.starts_with('/')
            || relative.contains("..")
            || relative.contains('\\')
            || relative.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(gateway_error("provider_bootstrap_path_invalid"));
        }
        self.api_root
            .join(relative)
            .map_err(|_| gateway_error("provider_bootstrap_url_invalid"))
    }

    async fn send(&self, method: Method, url: Url, body: Option<Vec<u8>>) -> Result<RawManagementResponse> {
        if !is_loopback_host(url.host()) {
            return Err(gateway_error("provider_bootstrap_non_loopback_forbidden"));
        }
        let mut request = self
            .http
            .request(method, url)
            .header("Accept", "application/json");
        if let Some(body) = body {
            if body.is_empty() || body.len() > 64 * 1024 {
                return Err(gateway_error("provider_bootstrap_request_size_invalid"));
            }
            request = request
                .header("Content-Type", "application/json")
                .body(body);
        }
        let mut response = request.send().await.map_err(map_reqwest_error)?;
        if let Some(length) = response.content_length() {
            if length > MAX_MANAGEMENT_RESPONSE_BYTES as u64 {
                return Err(gateway_error("provider_bootstrap_response_too_large"));
            }
        }
        let status = response.status().as_u16();
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
            let next = body
                .len()
                .checked_add(chunk.len())
                .ok_or_else(|| gateway_error("provider_bootstrap_response_too_large"))?;
            if next > MAX_MANAGEMENT_RESPONSE_BYTES {
                return Err(gateway_error("provider_bootstrap_response_too_large"));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(RawManagementResponse { status, body })
    }
}

struct RawManagementResponse {
    status: u16,
    body: Vec<u8>,
}

fn validate_connection_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(gateway_error("provider_bootstrap_connection_id_invalid"));
    }
    Ok(())
}

fn expect_success(status: u16, prefix: &'static str) -> Result<()> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(status_error(prefix, status))
    }
}

fn status_error(prefix: &'static str, status: u16) -> VibeCoderError {
    let class = match status {
        400 => "bad_request",
        401 => "authentication_required",
        403 => "access_denied",
        404 => "endpoint_not_found",
        409 => "conflict",
        429 => "rate_limited",
        500..=599 => "server_error",
        _ => "unexpected_status",
    };
    VibeCoderError::Gateway(format!("{prefix}_{class}"))
}

fn map_reqwest_error(error: reqwest::Error) -> VibeCoderError {
    let code = if error.is_timeout() {
        "provider_bootstrap_http_timeout"
    } else if error.is_connect() {
        "provider_bootstrap_http_connect_failed"
    } else if error.is_request() {
        "provider_bootstrap_http_request_failed"
    } else if error.is_body() {
        "provider_bootstrap_http_body_failed"
    } else {
        "provider_bootstrap_http_transport_failed"
    };
    gateway_error(code)
}

fn gateway_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Gateway(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    fn read_request(stream: &mut std::net::TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 4096];
        let mut content_length = None;
        loop {
            let read = stream.read(&mut buffer).unwrap();
            assert!(read > 0);
            bytes.extend_from_slice(&buffer[..read]);
            if content_length.is_none() {
                if let Some(split) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..split + 4]);
                    content_length = headers.lines().find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    });
                    if bytes.len() >= split + 4 + content_length.unwrap_or(0) {
                        break;
                    }
                }
            } else if let Some(split) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                if bytes.len() >= split + 4 + content_length.unwrap_or(0) {
                    break;
                }
            }
        }
        String::from_utf8(bytes).unwrap()
    }

    fn respond(stream: &mut std::net::TcpStream, status: &str, body: &str) {
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        stream.flush().unwrap();
    }

    #[test]
    fn creates_no_auth_free_provider_and_syncs_models() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured = Arc::clone(&requests);
        let server = thread::spawn(move || {
            let responses = [
                ("200 OK", r#"{"connections":[],"total":0}"#),
                ("201 Created", r#"{"connection":{"id":"conn-free","provider":"opencode","isActive":true}}"#),
                ("200 OK", r#"{"status":"synced"}"#),
            ];
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_request(&mut stream);
                captured.lock().unwrap().push(request);
                respond(&mut stream, status, body);
            }
        });

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap();
        let bootstrap = OmniRouteProviderBootstrap::new(OmniRouteConfig {
            base_url: format!("http://127.0.0.1:{}/v1", address.port()),
            request_timeout_ms: 5_000,
            max_response_bytes: 8 * 1024 * 1024,
        })
        .unwrap();
        let outcome = runtime
            .block_on(bootstrap.ensure_vibecoder_free_provider())
            .unwrap();
        assert_eq!(outcome, FreeProviderBootstrapOutcome::Created);
        server.join().unwrap();

        let requests = requests.lock().unwrap();
        assert!(requests[0].starts_with("GET /api/providers?provider=opencode&limit=10 "));
        assert!(requests[1].starts_with("POST /api/providers "));
        assert!(requests[1].contains(r#""provider":"opencode""#));
        assert!(requests[1].contains(r#""name":"OpenCode Free""#));
        assert!(!requests[1].contains("apiKey"));
        assert!(requests[2].starts_with("POST /api/providers/conn-free/sync-models?mode=import "));
    }

    #[test]
    fn automatic_provider_mutation_is_loopback_only() {
        let error = OmniRouteProviderBootstrap::new(OmniRouteConfig {
            base_url: "https://example.com/v1".into(),
            request_timeout_ms: 5_000,
            max_response_bytes: 8 * 1024 * 1024,
        })
        .unwrap_err();
        assert!(matches!(error, VibeCoderError::Gateway(code) if code == "provider_bootstrap_non_loopback_forbidden"));
    }

    #[test]
    fn connection_ids_cannot_escape_management_path() {
        assert!(validate_connection_id("conn-123_abc").is_ok());
        assert!(validate_connection_id("../escape").is_err());
        assert!(validate_connection_id("a/b").is_err());
    }

    #[test]
    fn create_payload_has_no_fake_secret() {
        let payload: serde_json::Value = json!({
            "provider": VIBECODER_FREE_PROVIDER_ID,
            "name": VIBECODER_FREE_PROVIDER_NAME,
            "priority": 1
        });
        assert_eq!(payload["provider"], "opencode");
        assert!(payload.get("apiKey").is_none());
    }
}
