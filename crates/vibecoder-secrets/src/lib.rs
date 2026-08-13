//! Provider-neutral secret-reference and resolution boundary.
//!
//! Persisted configuration contains only `SecretReference`. Resolved bytes are short lived,
//! non-serializable, redacted from `Debug`, and overwritten on drop. Android production will
//! provide an `AppSecureStore` resolver backed by Android Keystore-protected app-private storage;
//! environment lookup exists only as an explicit development/test source.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use vibecoder_domain::{Result, VibeCoderError};
use zeroize::Zeroize;

const MAX_SECRET_NAME_BYTES: usize = 128;
const MAX_SECRET_VALUE_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretSource {
    /// Android production source. The reference name is safe to persist; the value is not.
    AppSecureStore,
    /// Explicit development/test source. Never silently used for an AppSecureStore reference.
    Environment,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretReference {
    pub source: SecretSource,
    pub name: String,
}

impl SecretReference {
    pub fn validate(&self) -> Result<()> {
        validate_reference_name(self.source, &self.name)
    }
}

/// Resolved secret bytes.
///
/// Deliberately not `Clone`, `Serialize`, or `Deserialize`. Debug output is always redacted.
pub struct SecretValue {
    bytes: Vec<u8>,
}

impl SecretValue {
    pub fn new(bytes: Vec<u8>) -> Result<Self> {
        if bytes.is_empty() {
            return Err(secret_error("secret_value_empty"));
        }
        if bytes.len() > MAX_SECRET_VALUE_BYTES {
            return Err(secret_error("secret_value_too_large"));
        }
        Ok(Self { bytes })
    }

    pub fn from_utf8(value: String) -> Result<Self> {
        Self::new(value.into_bytes())
    }

    pub fn expose_str(&self) -> Result<&str> {
        std::str::from_utf8(&self.bytes).map_err(|_| secret_error("secret_value_not_utf8"))
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        // Zeroize uses operations designed not to be optimized away. This still cannot erase
        // copies made outside this owned buffer by the OS/runtime/provider stack.
        self.bytes.zeroize();
    }
}

#[async_trait]
pub trait SecretResolver: Send + Sync {
    async fn resolve(&self, reference: &SecretReference) -> Result<SecretValue>;
}

/// Platform boundary for the phone-local production secret store.
///
/// The future Android adapter will implement this with Keystore-protected app-private storage.
/// Returning `SecretValue` keeps the plaintext in the same short-lived/redacted type.
#[async_trait]
pub trait AppSecureStoreBackend: Send + Sync {
    async fn load_secret(&self, name: &str) -> Result<Option<SecretValue>>;
}

pub struct AppSecureStoreResolver<B> {
    backend: B,
}

impl<B> AppSecureStoreResolver<B> {
    pub const fn new(backend: B) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl<B> SecretResolver for AppSecureStoreResolver<B>
where
    B: AppSecureStoreBackend,
{
    async fn resolve(&self, reference: &SecretReference) -> Result<SecretValue> {
        reference.validate()?;
        if reference.source != SecretSource::AppSecureStore {
            return Err(secret_error(
                "secret_source_not_supported_by_secure_store_resolver",
            ));
        }
        self.backend
            .load_secret(&reference.name)
            .await
            .map_err(|_| secret_error("secure_store_backend_failed"))?
            .ok_or_else(|| secret_error("secret_reference_not_found"))
    }
}

/// Explicit environment resolver for development and tests.
///
/// It refuses `AppSecureStore` references instead of silently falling back to process env.
#[derive(Debug, Default, Clone, Copy)]
pub struct EnvironmentSecretResolver;

#[async_trait]
impl SecretResolver for EnvironmentSecretResolver {
    async fn resolve(&self, reference: &SecretReference) -> Result<SecretValue> {
        reference.validate()?;
        if reference.source != SecretSource::Environment {
            return Err(secret_error(
                "secret_source_not_supported_by_environment_resolver",
            ));
        }

        let value = std::env::var_os(&reference.name)
            .ok_or_else(|| secret_error("secret_reference_not_found"))?;
        let value = os_string_to_utf8(value)?;
        SecretValue::from_utf8(value)
    }
}

fn os_string_to_utf8(value: OsString) -> Result<String> {
    value
        .into_string()
        .map_err(|_| secret_error("secret_value_not_utf8"))
}

fn validate_reference_name(source: SecretSource, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_SECRET_NAME_BYTES
        || value.trim() != value
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(secret_error("secret_reference_name_invalid"));
    }

    match source {
        SecretSource::Environment => {
            let mut bytes = value.bytes();
            let Some(first) = bytes.next() else {
                return Err(secret_error("secret_reference_name_invalid"));
            };
            if !(first == b'_' || first.is_ascii_alphabetic())
                || !bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
            {
                return Err(secret_error("secret_reference_name_invalid"));
            }
        }
        SecretSource::AppSecureStore => {
            if !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
            }) {
                return Err(secret_error("secret_reference_name_invalid"));
            }
        }
    }
    Ok(())
}

fn secret_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Secret(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_value_debug_is_redacted() {
        let secret = SecretValue::from_utf8("super-secret-key".into()).unwrap();
        let debug = format!("{secret:?}");
        assert!(!debug.contains("super-secret-key"));
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn references_are_safe_and_source_specific() {
        let secure = SecretReference {
            source: SecretSource::AppSecureStore,
            name: "omniroute.api_key".into(),
        };
        assert!(secure.validate().is_ok());

        let bad_env = SecretReference {
            source: SecretSource::Environment,
            name: "BAD-NAME".into(),
        };
        assert!(bad_env.validate().is_err());
    }
}
