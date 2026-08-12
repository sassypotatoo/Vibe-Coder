use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use vibecoder_domain::{Result, VibeCoderError};

const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_STARTUP_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_CLEANUP_TIMEOUT_MS: u64 = 30_000;
const MAX_TIMEOUT_MS: u64 = 10 * 60 * 1_000;

/// How VibeCoder reaches a Jcode harness runtime.
///
/// `Shared` connects to an already-running/shared harness and can ask the Jcode SDK to start
/// the default runtime. `Private` launches an SDK-owned isolated runtime. The private mode is
/// the preferred foundation for later per-project isolation; Part 3 now assigns canonical project
/// working directories when creating sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case", deny_unknown_fields)]
pub enum JcodeConnectionMode {
    Shared {
        #[serde(default)]
        socket_path: Option<PathBuf>,
        #[serde(default = "default_true")]
        ensure_runtime: bool,
    },
    Private {
        #[serde(default)]
        jcode_home: Option<PathBuf>,
        #[serde(default)]
        binary: Option<PathBuf>,
        #[serde(default)]
        inherit_logins: bool,
        #[serde(default = "default_startup_timeout_ms")]
        startup_timeout_ms: u64,
        #[serde(default = "default_cleanup_timeout_ms")]
        cleanup_timeout_ms: u64,
    },
}

impl Default for JcodeConnectionMode {
    fn default() -> Self {
        Self::Private {
            jcode_home: None,
            binary: None,
            inherit_logins: false,
            startup_timeout_ms: DEFAULT_STARTUP_TIMEOUT_MS,
            cleanup_timeout_ms: DEFAULT_CLEANUP_TIMEOUT_MS,
        }
    }
}

/// Connection options that are safe to persist.
///
/// Provider/API secrets deliberately do not appear here. They belong to the secret-reference
/// work planned for Part 10.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JcodeConnectionConfig {
    #[serde(default = "default_client_name")]
    pub client_name: String,
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,
    #[serde(default)]
    pub connection: JcodeConnectionMode,
}

impl Default for JcodeConnectionConfig {
    fn default() -> Self {
        Self {
            client_name: default_client_name(),
            request_timeout_ms: DEFAULT_REQUEST_TIMEOUT_MS,
            connection: JcodeConnectionMode::default(),
        }
    }
}

impl JcodeConnectionConfig {
    pub fn validate(&self) -> Result<()> {
        let client_name = self.client_name.trim();
        if client_name.is_empty() {
            return Err(VibeCoderError::InvalidRequest(
                "Jcode client_name cannot be empty".into(),
            ));
        }
        if client_name.len() > 128 {
            return Err(VibeCoderError::InvalidRequest(
                "Jcode client_name cannot exceed 128 bytes".into(),
            ));
        }
        validate_timeout("request_timeout_ms", self.request_timeout_ms)?;

        match &self.connection {
            JcodeConnectionMode::Shared {
                socket_path,
                ensure_runtime,
            } => {
                // Jcode SDK's shared `connect()` autostart currently boots the default API
                // socket. Combining that with a custom socket path can therefore start one
                // runtime and then dial another path. Fail closed instead of creating a
                // confusing split-brain startup path.
                if let Some(path) = socket_path {
                    if path.as_os_str().is_empty() {
                        return Err(VibeCoderError::InvalidRequest(
                            "Jcode socket_path cannot be empty".into(),
                        ));
                    }
                    if *ensure_runtime {
                        return Err(VibeCoderError::InvalidRequest(
                            "custom Jcode socket_path requires ensure_runtime=false".into(),
                        ));
                    }
                }
            }
            JcodeConnectionMode::Private {
                jcode_home,
                binary,
                startup_timeout_ms,
                cleanup_timeout_ms,
                ..
            } => {
                if jcode_home
                    .as_ref()
                    .is_some_and(|path| path.as_os_str().is_empty())
                {
                    return Err(VibeCoderError::InvalidRequest(
                        "Jcode jcode_home cannot be empty".into(),
                    ));
                }
                if binary
                    .as_ref()
                    .is_some_and(|path| path.as_os_str().is_empty())
                {
                    return Err(VibeCoderError::InvalidRequest(
                        "Jcode binary path cannot be empty".into(),
                    ));
                }
                validate_timeout("startup_timeout_ms", *startup_timeout_ms)?;
                validate_timeout("cleanup_timeout_ms", *cleanup_timeout_ms)?;
            }
        }
        Ok(())
    }

    pub(crate) fn request_timeout(&self) -> Duration {
        Duration::from_millis(self.request_timeout_ms)
    }
}

fn validate_timeout(name: &str, value: u64) -> Result<()> {
    if value == 0 {
        return Err(VibeCoderError::InvalidRequest(format!(
            "{name} must be greater than zero"
        )));
    }
    if value > MAX_TIMEOUT_MS {
        return Err(VibeCoderError::InvalidRequest(format!(
            "{name} cannot exceed {MAX_TIMEOUT_MS} ms"
        )));
    }
    Ok(())
}

fn default_true() -> bool {
    true
}

fn default_client_name() -> String {
    format!("vibecoder/{}", env!("CARGO_PKG_VERSION"))
}

fn default_request_timeout_ms() -> u64 {
    DEFAULT_REQUEST_TIMEOUT_MS
}

fn default_startup_timeout_ms() -> u64 {
    DEFAULT_STARTUP_TIMEOUT_MS
}

fn default_cleanup_timeout_ms() -> u64 {
    DEFAULT_CLEANUP_TIMEOUT_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_shared_socket_must_not_autostart_default_runtime() {
        let config = JcodeConnectionConfig {
            connection: JcodeConnectionMode::Shared {
                socket_path: Some(PathBuf::from("/tmp/custom-jcode.sock")),
                ensure_runtime: true,
            },
            ..JcodeConnectionConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn private_defaults_validate() {
        assert!(JcodeConnectionConfig::default().validate().is_ok());
    }
}
