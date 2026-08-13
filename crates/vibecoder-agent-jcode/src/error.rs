use jcode_sdk::{Error as SdkError, ErrorKind as SdkErrorKind};
use serde::{Deserialize, Serialize};
use vibecoder_domain::VibeCoderError;

/// Broad failure class used by orchestration/retry policy without matching Jcode prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JcodeFailureClass {
    RetryableTransport,
    RuntimeUnavailable,
    ProtocolMismatch,
    InvalidConfiguration,
    RemoteFailure,
    FatalTransport,
}

/// Sanitized connection failure suitable for persisted lifecycle state and ordinary UI/status use.
///
/// Raw SDK prose is intentionally not persisted here: Jcode startup errors may include captured
/// process stderr or host paths. A future diagnostics layer can retain redacted raw details in a
/// non-project, access-controlled sink without widening this public state object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JcodeConnectionFailure {
    pub code: String,
    pub message: String,
    pub class: JcodeFailureClass,
    pub retryable: bool,
}

impl JcodeConnectionFailure {
    pub(crate) fn from_sdk(error: SdkError) -> Self {
        let code = error.code().to_string();
        let class = classify(&error.kind);
        let retryable = is_retryable(&error.kind);
        Self {
            code,
            message: safe_message(&error.kind).to_string(),
            class,
            retryable,
        }
    }

    pub(crate) fn into_domain_error(self) -> VibeCoderError {
        VibeCoderError::Agent(format!("{}: {}", self.code, self.message))
    }
}

fn classify(kind: &SdkErrorKind) -> JcodeFailureClass {
    match kind {
        SdkErrorKind::ConnectFailed
        | SdkErrorKind::Timeout
        | SdkErrorKind::Disconnected
        | SdkErrorKind::Transport => JcodeFailureClass::RetryableTransport,
        SdkErrorKind::LaunchFailed
        | SdkErrorKind::JcodeNotFound
        | SdkErrorKind::StartupFailed
        | SdkErrorKind::StartupTimeout => JcodeFailureClass::RuntimeUnavailable,
        SdkErrorKind::HandshakeFailed
        | SdkErrorKind::Harness(jcode_sdk::api::ErrorCode::UnsupportedVersion) => {
            JcodeFailureClass::ProtocolMismatch
        }
        SdkErrorKind::InvalidInstanceHome | SdkErrorKind::InvalidOption => {
            JcodeFailureClass::InvalidConfiguration
        }
        SdkErrorKind::Harness(_) | SdkErrorKind::UnexpectedReply => {
            JcodeFailureClass::RemoteFailure
        }
        SdkErrorKind::UnsupportedTransport | SdkErrorKind::EventBufferOverflow => {
            JcodeFailureClass::FatalTransport
        }
    }
}

fn is_retryable(kind: &SdkErrorKind) -> bool {
    matches!(
        kind,
        SdkErrorKind::ConnectFailed
            | SdkErrorKind::Timeout
            | SdkErrorKind::Disconnected
            | SdkErrorKind::Transport
            | SdkErrorKind::LaunchFailed
            | SdkErrorKind::StartupFailed
            | SdkErrorKind::StartupTimeout
    )
}

fn safe_message(kind: &SdkErrorKind) -> &'static str {
    match kind {
        SdkErrorKind::ConnectFailed => "Could not connect to the Jcode harness",
        SdkErrorKind::HandshakeFailed => "Jcode harness handshake failed",
        SdkErrorKind::Timeout => "Jcode harness request timed out",
        SdkErrorKind::Disconnected => "Jcode harness connection closed",
        SdkErrorKind::UnexpectedReply => "Jcode harness returned an unexpected reply",
        SdkErrorKind::Transport => "Jcode harness transport failed",
        SdkErrorKind::LaunchFailed => "Jcode runtime could not be launched",
        SdkErrorKind::JcodeNotFound => "Jcode runtime executable is unavailable",
        SdkErrorKind::StartupFailed => "Jcode runtime exited during startup",
        SdkErrorKind::StartupTimeout => "Jcode runtime did not become ready in time",
        SdkErrorKind::InvalidInstanceHome => "Jcode runtime state directory is invalid",
        SdkErrorKind::InvalidOption => "Jcode runtime configuration is invalid",
        SdkErrorKind::UnsupportedTransport => "Jcode transport does not support this operation",
        SdkErrorKind::EventBufferOverflow => "Jcode event consumer exceeded its buffer",
        SdkErrorKind::Harness(jcode_sdk::api::ErrorCode::UnsupportedVersion) => {
            "Jcode harness protocol version is incompatible"
        }
        SdkErrorKind::Harness(_) => "Jcode harness rejected the operation",
    }
}

pub(crate) fn map_operation_error(operation: &'static str, error: SdkError) -> VibeCoderError {
    let code = error.code();
    match &error.kind {
        SdkErrorKind::Harness(jcode_sdk::api::ErrorCode::UnknownSession) => {
            VibeCoderError::InvalidRequest(format!(
                "{operation}: Jcode session does not exist ({code})"
            ))
        }
        SdkErrorKind::Harness(jcode_sdk::api::ErrorCode::InvalidRequest)
        | SdkErrorKind::InvalidOption => VibeCoderError::InvalidRequest(format!(
            "{operation}: Jcode rejected the request ({code})"
        )),
        _ => VibeCoderError::Agent(format!(
            "{operation}: {} ({code})",
            safe_message(&error.kind)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_sdk_message_is_not_copied_into_persisted_failure() {
        let failure = JcodeConnectionFailure::from_sdk(SdkError::new(
            SdkErrorKind::StartupFailed,
            "SECRET_TOKEN=should-not-escape",
        ));
        assert!(!failure.message.contains("SECRET_TOKEN"));
        assert_eq!(failure.code, "startup_failed");
    }
}
