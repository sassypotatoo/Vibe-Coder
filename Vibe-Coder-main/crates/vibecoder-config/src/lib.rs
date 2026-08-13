//! Strict persisted application configuration.
//!
//! Configuration is data, never a secret store. The loader accepts only bounded JSON, rejects
//! common plaintext credential fields before typed deserialization, and returns sanitized stable
//! error codes. Persisted authentication is represented only by `SecretReference`.

use serde::de::{Error as DeError, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::fmt;
use std::path::PathBuf;
use vibecoder_agent_jcode::JcodeConnectionConfig;
use vibecoder_domain::{Result, VibeCoderError};
use vibecoder_gateway_omniroute::OmniRouteConfig;
use vibecoder_routing::ModelRoutePolicyConfig;
use vibecoder_secrets::SecretReference;

const MAX_CONFIG_BYTES: usize = 256 * 1024;
const MAX_JSON_DEPTH: usize = 32;
const DUPLICATE_KEY_SENTINEL: &str = "vibecoder_duplicate_json_key";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    JcodeHarness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSection {
    pub kind: AgentKind,
    pub config: JcodeConnectionConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelGatewayKind {
    #[serde(rename = "omniroute")]
    OmniRoute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelGatewaySection {
    pub kind: ModelGatewayKind,
    pub config: OmniRouteConfig,
    #[serde(default)]
    pub credential_ref: Option<SecretReference>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceKind {
    Isolated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSection {
    pub kind: WorkspaceKind,
    pub root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendConfig {
    pub agent: AgentSection,
    pub model_gateway: ModelGatewaySection,
    pub workspace: WorkspaceSection,
    pub routing: ModelRoutePolicyConfig,
}

impl BackendConfig {
    pub fn validate(&self) -> Result<()> {
        self.agent.config.validate()?;
        self.model_gateway.config.validate()?;
        if let Some(reference) = &self.model_gateway.credential_ref {
            reference.validate()?;
        }
        if self.workspace.root.as_os_str().is_empty() {
            return Err(config_error("workspace_root_empty"));
        }
        self.routing.validate()?;
        Ok(())
    }

    pub fn gateway_credential_reference(&self) -> Option<&SecretReference> {
        self.model_gateway.credential_ref.as_ref()
    }
}

pub fn load_backend_config_json(raw: &[u8]) -> Result<BackendConfig> {
    if raw.is_empty() {
        return Err(config_error("config_empty"));
    }
    if raw.len() > MAX_CONFIG_BYTES {
        return Err(config_error("config_too_large"));
    }

    let mut deserializer = serde_json::Deserializer::from_slice(raw);
    let value = StrictValue::deserialize(&mut deserializer)
        .map_err(|error| map_json_error(&error.to_string()))?
        .0;
    deserializer
        .end()
        .map_err(|_| config_error("config_invalid_json"))?;
    validate_json_tree(&value, 0)?;
    reject_plaintext_secret_fields(&value)?;

    let config: BackendConfig =
        serde_json::from_value(value).map_err(|_| config_error("config_schema_invalid"))?;
    config.validate()?;
    Ok(config)
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictValueVisitor)
    }
}

struct StrictValueVisitor;

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = StrictValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> std::result::Result<Self::Value, E> {
        Ok(StrictValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> std::result::Result<Self::Value, E>
    where
        E: DeError,
    {
        let number =
            serde_json::Number::from_f64(value).ok_or_else(|| E::custom("invalid JSON number"))?;
        Ok(StrictValue(Value::Number(number)))
    }

    fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<StrictValue>()? {
            values.push(value.0);
        }
        Ok(StrictValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut object = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(A::Error::custom(DUPLICATE_KEY_SENTINEL));
            }
            let value = map.next_value::<StrictValue>()?.0;
            object.insert(key, value);
        }
        Ok(StrictValue(Value::Object(object)))
    }
}

fn map_json_error(message: &str) -> VibeCoderError {
    if message.contains(DUPLICATE_KEY_SENTINEL) {
        config_error("config_duplicate_json_key")
    } else {
        config_error("config_invalid_json")
    }
}

fn validate_json_tree(value: &Value, depth: usize) -> Result<()> {
    if depth > MAX_JSON_DEPTH {
        return Err(config_error("config_nesting_too_deep"));
    }
    match value {
        Value::Array(items) => {
            for item in items {
                validate_json_tree(item, depth + 1)?;
            }
        }
        Value::Object(map) => {
            for value in map.values() {
                validate_json_tree(value, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn reject_plaintext_secret_fields(value: &Value) -> Result<()> {
    match value {
        Value::Array(items) => {
            for item in items {
                reject_plaintext_secret_fields(item)?;
            }
        }
        Value::Object(map) => {
            for (key, nested) in map {
                if is_forbidden_plaintext_secret_key(key) {
                    return Err(config_error("plaintext_secret_field_forbidden"));
                }
                reject_plaintext_secret_fields(nested)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_forbidden_plaintext_secret_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "api_key"
            | "apikey"
            | "access_token"
            | "refresh_token"
            | "bearer_token"
            | "auth_token"
            | "authorization"
            | "credentials"
            | "password"
            | "passwd"
            | "secret"
            | "client_secret"
            | "private_key"
    )
}

fn config_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Config(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_plaintext_api_key_before_schema_mapping() {
        let raw = br#"{"api_key":"must-never-persist"}"#;
        let error = load_backend_config_json(raw).unwrap_err().to_string();
        assert!(error.contains("plaintext_secret_field_forbidden"));
        assert!(!error.contains("must-never-persist"));
    }

    #[test]
    fn malformed_json_error_never_echoes_input() {
        let raw = br#"{"password":"super-secret", BROKEN"#;
        let error = load_backend_config_json(raw).unwrap_err().to_string();
        assert!(error.contains("config_invalid_json"));
        assert!(!error.contains("super-secret"));
    }
    #[test]
    fn rejects_duplicate_object_keys() {
        let raw = br#"{"workspace":{},"workspace":{}}"#;
        let error = load_backend_config_json(raw).unwrap_err().to_string();
        assert!(error.contains("config_duplicate_json_key"));
    }

    #[test]
    fn checked_in_example_is_valid_and_uses_only_a_reference() {
        let raw = include_bytes!("../../../config/backend.example.json");
        let config = load_backend_config_json(raw).unwrap();
        let reference = config.gateway_credential_reference().unwrap();
        assert_eq!(reference.name, "omniroute.api_key");
    }
}
