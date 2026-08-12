use vibecoder_domain::{Result, VibeCoderError};
pub use vibecoder_gateway_contract::GatewayCredential as RequestAuth;

const MAX_BEARER_TOKEN_BYTES: usize = 8192;

/// Backwards-compatible concrete transport name. The value itself lives in the provider-neutral
/// contract so the core can pass a borrowed credential without storing it.
pub(crate) fn bearer_token<'a>(auth: RequestAuth<'a>) -> Result<Option<&'a str>> {
    match auth {
        RequestAuth::Anonymous => Ok(None),
        RequestAuth::Secret(token) => {
            validate_bearer_token(token)?;
            Ok(Some(token))
        }
    }
}

fn validate_bearer_token(token: &str) -> Result<()> {
    if token.is_empty()
        || token.len() > MAX_BEARER_TOKEN_BYTES
        || token.trim() != token
        || !token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(VibeCoderError::InvalidRequest(
            "OmniRoute bearer token has an invalid HTTP credential shape".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_reveals_bearer_token() {
        let debug = format!("{:?}", RequestAuth::Secret("super-secret-key"));
        assert!(!debug.contains("super-secret-key"));
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn rejects_whitespace_and_control_characters() {
        assert!(bearer_token(RequestAuth::Secret(" key")).is_err());
        assert!(bearer_token(RequestAuth::Secret("key value")).is_err());
        assert!(bearer_token(RequestAuth::Secret("key\nvalue")).is_err());
    }
}
