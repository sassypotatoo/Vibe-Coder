use crate::auth::{RequestAuth, bearer_token};
use crate::config::{OmniRouteConfig, ValidatedOmniRouteConfig};
use reqwest::{Client, Method, StatusCode};
use std::time::Duration;
use url::Url;
use vibecoder_domain::{Result, VibeCoderError};

const USER_AGENT: &str = concat!("vibecoder/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Copy)]
enum ApiEndpoint {
    Models,
    RuntimeProfile,
}

impl ApiEndpoint {
    fn relative_path(self) -> &'static str {
        match self {
            Self::Models => "models",
            Self::RuntimeProfile => "vibecoder/runtime-profile",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RawGatewayResponse {
    pub(crate) status: u16,
    pub(crate) content_type: Option<String>,
    pub(crate) body: Vec<u8>,
}

/// Thin, replaceable HTTP boundary for the OmniRoute OpenAI-compatible API.
///
/// Part 7 intentionally does not interpret health or model-catalog JSON. Those semantics belong to
/// Part 8. This type owns only validated endpoint construction, transport policy, bounded response
/// collection, and ephemeral Bearer-header injection.
#[derive(Clone)]
pub struct OmniRouteClient {
    http: Client,
    api_base: Url,
    max_response_bytes: usize,
}

impl std::fmt::Debug for OmniRouteClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OmniRouteClient")
            .field("api_base", &self.api_base.as_str())
            .field("max_response_bytes", &self.max_response_bytes)
            .finish_non_exhaustive()
    }
}

impl OmniRouteClient {
    pub fn new(config: OmniRouteConfig) -> Result<Self> {
        let validated = config.validated()?;
        Self::from_validated(validated)
    }

    fn from_validated(config: ValidatedOmniRouteConfig) -> Result<Self> {
        let http = Client::builder()
            .connect_timeout(Duration::from_millis(config.request_timeout_ms))
            .timeout(Duration::from_millis(config.request_timeout_ms))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .user_agent(USER_AGENT)
            .build()
            .map_err(|_| gateway_error("http_client_build_failed"))?;

        Ok(Self {
            http,
            api_base: config.api_base,
            max_response_bytes: config.max_response_bytes,
        })
    }

    pub fn api_base_url(&self) -> &str {
        self.api_base.as_str()
    }

    pub(crate) async fn get_models_raw(&self, auth: RequestAuth<'_>) -> Result<RawGatewayResponse> {
        self.send_bounded(Method::GET, ApiEndpoint::Models, auth)
            .await
    }

    pub(crate) async fn get_runtime_profile_raw(
        &self,
        auth: RequestAuth<'_>,
    ) -> Result<RawGatewayResponse> {
        self.send_bounded(Method::GET, ApiEndpoint::RuntimeProfile, auth)
            .await
    }

    /// OmniRoute 3.8.50 implements HEAD /v1/models as an unconditional availability probe.
    /// It must never be treated as proof that Bearer auth or catalog access succeeded.
    #[allow(
        dead_code,
        reason = "retained as the explicitly anonymous availability-only transport boundary"
    )]
    pub(crate) async fn head_models_availability_raw(&self) -> Result<RawGatewayResponse> {
        // Upstream HEAD is intentionally unauthenticated, so never send a Bearer token here.
        self.send_bounded(Method::HEAD, ApiEndpoint::Models, RequestAuth::Anonymous)
            .await
    }

    fn endpoint_url(&self, endpoint: ApiEndpoint) -> Result<Url> {
        self.api_base
            .join(endpoint.relative_path())
            .map_err(|_| gateway_error("endpoint_join_failed"))
    }

    async fn send_bounded(
        &self,
        method: Method,
        endpoint: ApiEndpoint,
        auth: RequestAuth<'_>,
    ) -> Result<RawGatewayResponse> {
        let url = self.endpoint_url(endpoint)?;
        let is_head = method == Method::HEAD;
        let mut request = self
            .http
            .request(method, url)
            .header("Accept", "application/json");
        if let Some(token) = bearer_token(auth)? {
            request = request.bearer_auth(token);
        }

        let mut response = request.send().await.map_err(map_reqwest_error)?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);

        if let Some(length) = response.content_length() {
            if length > self.max_response_bytes as u64 {
                return Err(gateway_error("response_too_large"));
            }
        }

        let body = if status == StatusCode::NO_CONTENT || is_head {
            Vec::new()
        } else {
            read_bounded_body(&mut response, self.max_response_bytes).await?
        };

        Ok(RawGatewayResponse {
            status: status.as_u16(),
            content_type,
            body,
        })
    }
}

async fn read_bounded_body(response: &mut reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
        let next_len = body
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| gateway_error("response_too_large"))?;
        if next_len > limit {
            return Err(gateway_error("response_too_large"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn map_reqwest_error(error: reqwest::Error) -> VibeCoderError {
    let code = if error.is_timeout() {
        "http_timeout"
    } else if error.is_connect() {
        "http_connect_failed"
    } else if error.is_request() {
        "http_request_failed"
    } else if error.is_body() {
        "http_body_failed"
    } else {
        "http_transport_failed"
    };
    gateway_error(code)
}

fn gateway_error(code: &'static str) -> VibeCoderError {
    // Do not persist reqwest error prose here. It can contain endpoint details and future
    // authentication context. Stable codes are enough for orchestration; diagnostics can be added
    // later through an explicitly redacted logging boundary.
    VibeCoderError::Gateway(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_join_never_duplicates_v1() {
        let config = OmniRouteConfig {
            base_url: "http://127.0.0.1:20128/v1".into(),
            request_timeout_ms: 30_000,
            max_response_bytes: 8 * 1024 * 1024,
        };
        let client = OmniRouteClient::new(config).unwrap();
        assert_eq!(
            client.endpoint_url(ApiEndpoint::Models).unwrap().as_str(),
            "http://127.0.0.1:20128/v1/models"
        );
        assert_eq!(
            client
                .endpoint_url(ApiEndpoint::RuntimeProfile)
                .unwrap()
                .as_str(),
            "http://127.0.0.1:20128/v1/vibecoder/runtime-profile"
        );
    }
}
