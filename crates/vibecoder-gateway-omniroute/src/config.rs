use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, Ipv6Addr};
use url::{Host, Url};
use vibecoder_domain::{Result, VibeCoderError};
use vibecoder_gateway_contract::GatewayConfig;

const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 30_000;
const MIN_REQUEST_TIMEOUT_MS: u64 = 100;
const MAX_REQUEST_TIMEOUT_MS: u64 = 300_000;
const DEFAULT_MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MIN_MAX_RESPONSE_BYTES: usize = 1024;
const MAX_MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

fn default_request_timeout_ms() -> u64 {
    DEFAULT_REQUEST_TIMEOUT_MS
}

fn default_max_response_bytes() -> usize {
    DEFAULT_MAX_RESPONSE_BYTES
}

/// Configuration for the OmniRoute OpenAI-compatible API boundary.
///
/// Authentication is intentionally absent from this transport configuration. Part 10 keeps
/// persisted credential references in the application config/secrets layer, never in this client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OmniRouteConfig {
    pub base_url: String,
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,
    #[serde(default = "default_max_response_bytes")]
    pub max_response_bytes: usize,
}

impl From<GatewayConfig> for OmniRouteConfig {
    fn from(value: GatewayConfig) -> Self {
        Self {
            base_url: value.base_url,
            request_timeout_ms: DEFAULT_REQUEST_TIMEOUT_MS,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ValidatedOmniRouteConfig {
    pub(crate) api_base: Url,
    pub(crate) request_timeout_ms: u64,
    pub(crate) max_response_bytes: usize,
}

impl OmniRouteConfig {
    pub fn validate(&self) -> Result<()> {
        self.validated().map(|_| ())
    }

    pub(crate) fn validated(&self) -> Result<ValidatedOmniRouteConfig> {
        if self.request_timeout_ms < MIN_REQUEST_TIMEOUT_MS
            || self.request_timeout_ms > MAX_REQUEST_TIMEOUT_MS
        {
            return Err(invalid(
                "OmniRoute request_timeout_ms is outside the allowed range",
            ));
        }
        if self.max_response_bytes < MIN_MAX_RESPONSE_BYTES
            || self.max_response_bytes > MAX_MAX_RESPONSE_BYTES
        {
            return Err(invalid(
                "OmniRoute max_response_bytes is outside the allowed range",
            ));
        }

        let api_base = validate_and_normalize_base_url(&self.base_url)?;

        Ok(ValidatedOmniRouteConfig {
            api_base,
            request_timeout_ms: self.request_timeout_ms,
            max_response_bytes: self.max_response_bytes,
        })
    }
}

fn validate_and_normalize_base_url(raw: &str) -> Result<Url> {
    if raw.is_empty()
        || raw.len() > 2048
        || raw
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(invalid(
            "OmniRoute base_url must be a bounded URL without whitespace or control characters",
        ));
    }
    if raw.contains('\\') || has_raw_dot_segment(raw) {
        return Err(invalid(
            "OmniRoute base_url contains a non-canonical path representation",
        ));
    }
    let authority = raw_authority(raw)
        .ok_or_else(|| invalid("OmniRoute base_url must contain a canonical // authority"))?;
    if authority.is_empty() {
        return Err(invalid(
            "OmniRoute base_url must contain a non-empty URL authority",
        ));
    }
    if authority.contains('@') {
        return Err(invalid("OmniRoute base_url must not contain URL user-info"));
    }

    let mut url =
        Url::parse(raw).map_err(|_| invalid("OmniRoute base_url is not a valid absolute URL"))?;
    if url.cannot_be_a_base() || url.host().is_none() {
        return Err(invalid(
            "OmniRoute base_url must contain an absolute network host",
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid(
            "OmniRoute base_url must not contain username or password credentials",
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(invalid(
            "OmniRoute base_url must not contain a query string or fragment",
        ));
    }
    if url.path().contains('%') {
        return Err(invalid(
            "OmniRoute base_url path must not rely on percent-encoded segments",
        ));
    }
    if url.port() == Some(0) {
        return Err(invalid("OmniRoute base_url must not use network port 0"));
    }

    match url.scheme() {
        "https" => {}
        "http" if is_loopback_host(url.host()) => {}
        "http" => {
            return Err(invalid(
                "plain HTTP is allowed only for a loopback OmniRoute host; remote gateways require HTTPS",
            ));
        }
        _ => return Err(invalid("OmniRoute base_url must use http:// or https://")),
    }

    let path = url.path();
    let normalized_path = if path == "/" || path.is_empty() {
        "/v1/".to_owned()
    } else if path.ends_with("/v1") {
        format!("{path}/")
    } else if path.ends_with("/v1/") {
        path.to_owned()
    } else {
        return Err(invalid(
            "OmniRoute base_url must be the API root ending in /v1 (or a bare origin)",
        ));
    };

    // Empty and dot path segments make endpoint joining ambiguous. Url parsing resolves literal
    // dot segments, so this catches the remaining duplicate-slash form before we canonicalize.
    if normalized_path != "/v1/" && normalized_path[..normalized_path.len() - 1].contains("//") {
        return Err(invalid(
            "OmniRoute base_url contains ambiguous empty path segments",
        ));
    }

    url.set_path(&normalized_path);
    Ok(url)
}

fn raw_authority(raw: &str) -> Option<&str> {
    let (_, after_scheme) = raw.split_once("://")?;
    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    Some(&after_scheme[..authority_end])
}

fn has_raw_dot_segment(raw: &str) -> bool {
    let pathish = raw.split_once("://").map(|(_, rest)| rest).unwrap_or(raw);
    let path_start = pathish.find('/');
    let Some(path_start) = path_start else {
        return false;
    };
    let path = &pathish[path_start..];
    path == "/."
        || path == "/.."
        || path.ends_with("/.")
        || path.ends_with("/..")
        || path.contains("/./")
        || path.contains("/../")
}

fn is_loopback_host(host: Option<Host<&str>>) -> bool {
    match host {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => is_ipv4_loopback(address),
        Some(Host::Ipv6(address)) => is_ipv6_loopback(address),
        None => false,
    }
}

fn is_ipv4_loopback(address: Ipv4Addr) -> bool {
    address.is_loopback()
}

fn is_ipv6_loopback(address: Ipv6Addr) -> bool {
    address.is_loopback()
}

fn invalid(message: &'static str) -> VibeCoderError {
    VibeCoderError::InvalidRequest(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(url: &str) -> OmniRouteConfig {
        OmniRouteConfig {
            base_url: url.into(),
            request_timeout_ms: DEFAULT_REQUEST_TIMEOUT_MS,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        }
    }

    #[test]
    fn accepts_loopback_http_and_canonicalizes_v1() {
        let validated = config("http://127.0.0.1:20128/v1").validated().unwrap();
        assert_eq!(validated.api_base.as_str(), "http://127.0.0.1:20128/v1/");
    }

    #[test]
    fn accepts_https_subpath_api_root() {
        let validated = config("https://example.test/omniroute/v1")
            .validated()
            .unwrap();
        assert_eq!(
            validated.api_base.as_str(),
            "https://example.test/omniroute/v1/"
        );
    }

    #[test]
    fn rejects_remote_plain_http() {
        assert!(config("http://example.test/v1").validate().is_err());
    }

    #[test]
    fn rejects_embedded_credentials_query_and_fragment() {
        assert!(
            config("https://user:secret@example.test/v1")
                .validate()
                .is_err()
        );
        assert!(config("https://@example.test/v1").validate().is_err());
        assert!(
            config("https://example.test/v1?token=secret")
                .validate()
                .is_err()
        );
        assert!(
            config("https://example.test/v1#fragment")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn rejects_parser_normalization_tricks() {
        assert!(config("https:///example.test/v1").validate().is_err());
        assert!(config("https://example.test/a/../v1").validate().is_err());
        assert!(config("https://example.test/%76%31").validate().is_err());
        assert!(config("https://example.test\\v1").validate().is_err());
        assert!(config("https://example.test/v1\n").validate().is_err());
    }

    #[test]
    fn rejects_endpoint_instead_of_api_root() {
        assert!(config("https://example.test/v1/models").validate().is_err());
    }
}
