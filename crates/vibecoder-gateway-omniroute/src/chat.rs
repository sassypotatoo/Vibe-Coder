use crate::catalog::is_json_content_type;
use crate::client::{OmniRouteClient, RawGatewayResponse};
use serde::{Deserialize, Serialize};
use vibecoder_domain::{Result, TokenUsage, VibeCoderError};
use vibecoder_gateway_contract::{
    GatewayChatRequest, GatewayChatResponse, GatewayChatRole, GatewayCredential,
};

const MAX_MODEL_ID_BYTES: usize = 512;
const MAX_PROVIDER_BYTES: usize = 256;
const MAX_MESSAGES: usize = 64;
const MAX_MESSAGE_BYTES: usize = 128 * 1024;
const MAX_TOTAL_MESSAGE_BYTES: usize = 256 * 1024;
const MAX_OUTPUT_TOKENS: u32 = 8192;
const MAX_FINISH_REASON_BYTES: usize = 64;

#[derive(Debug, Serialize)]
struct OpenAiChatRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAiChatMessage<'a>>,
    stream: bool,
    max_tokens: u32,
}

#[derive(Debug, Serialize)]
struct OpenAiChatMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatEnvelope {
    #[serde(default)]
    model: Option<String>,
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    index: usize,
    message: OpenAiAssistantMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiAssistantMessage {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<serde_json::Value>,
    #[serde(default)]
    function_call: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
    #[serde(default)]
    prompt_tokens_details: Option<OpenAiPromptTokenDetails>,
}

#[derive(Debug, Deserialize)]
struct OpenAiPromptTokenDetails {
    #[serde(default)]
    cached_tokens: Option<u64>,
}

pub(crate) async fn execute_chat_completion(
    client: &OmniRouteClient,
    credential: GatewayCredential<'_>,
    request: &GatewayChatRequest,
) -> Result<GatewayChatResponse> {
    validate_request(request)?;
    let wire = build_wire_request(request);
    let body = serde_json::to_vec(&wire).map_err(|_| gateway_error("chat_request_json_failed"))?;
    let raw = client.post_chat_completion_raw(credential, &body).await?;
    interpret_chat_response(raw, credential, &request.model.id)
}

fn validate_request(request: &GatewayChatRequest) -> Result<()> {
    validate_bounded_text(&request.model.id, MAX_MODEL_ID_BYTES, "invalid_inference_model_id")?;
    if let Some(provider) = request.model.provider.as_deref() {
        validate_bounded_text(provider, MAX_PROVIDER_BYTES, "invalid_inference_model_provider")?;
    }
    if request.messages.is_empty() || request.messages.len() > MAX_MESSAGES {
        return Err(gateway_error("invalid_inference_message_count"));
    }
    if request.max_output_tokens == 0 || request.max_output_tokens > MAX_OUTPUT_TOKENS {
        return Err(gateway_error("invalid_inference_max_output_tokens"));
    }

    let mut total = 0usize;
    let mut has_user = false;
    for message in &request.messages {
        if matches!(message.role, GatewayChatRole::User) {
            has_user = true;
        }
        if message.content.is_empty()
            || message.content.len() > MAX_MESSAGE_BYTES
            || message.content.contains('\0')
        {
            return Err(gateway_error("invalid_inference_message_content"));
        }
        total = total
            .checked_add(message.content.len())
            .ok_or_else(|| gateway_error("inference_request_too_large"))?;
        if total > MAX_TOTAL_MESSAGE_BYTES {
            return Err(gateway_error("inference_request_too_large"));
        }
    }
    if !has_user {
        return Err(gateway_error("inference_user_message_required"));
    }
    Ok(())
}

fn build_wire_request(request: &GatewayChatRequest) -> OpenAiChatRequest<'_> {
    OpenAiChatRequest {
        model: &request.model.id,
        messages: request
            .messages
            .iter()
            .map(|message| OpenAiChatMessage {
                role: match message.role {
                    GatewayChatRole::System => "system",
                    GatewayChatRole::User => "user",
                    GatewayChatRole::Assistant => "assistant",
                },
                content: &message.content,
            })
            .collect(),
        stream: false,
        max_tokens: request.max_output_tokens,
    }
}

fn interpret_chat_response(
    response: RawGatewayResponse,
    credential: GatewayCredential<'_>,
    requested_model_id: &str,
) -> Result<GatewayChatResponse> {
    if response.status != 200 {
        return Err(gateway_error(match response.status {
            400 | 409 | 413 | 422 => "inference_invalid_request",
            401 if credential.is_anonymous() => "inference_authentication_required",
            401 => "inference_authentication_rejected",
            403 => "inference_access_denied",
            404 => "inference_model_or_endpoint_not_found",
            408 => "inference_timeout",
            429 => "inference_rate_limited",
            500..=599 => "inference_provider_unavailable",
            _ => "unexpected_inference_status",
        }));
    }
    if !is_json_content_type(response.content_type.as_deref()) {
        return Err(gateway_error("invalid_inference_content_type"));
    }
    if response.body.is_empty() {
        return Err(gateway_error("empty_inference_response"));
    }

    let envelope: OpenAiChatEnvelope = serde_json::from_slice(&response.body)
        .map_err(|_| gateway_error("invalid_inference_json"))?;
    if envelope.choices.len() != 1 || envelope.choices[0].index != 0 {
        return Err(gateway_error("invalid_inference_choices"));
    }
    let choice = &envelope.choices[0];
    if choice.message.role.as_deref().is_some_and(|role| role != "assistant") {
        return Err(gateway_error("invalid_inference_message_role"));
    }
    if choice.message.tool_calls.as_ref().is_some_and(nonempty_json_value)
        || choice.message.function_call.as_ref().is_some_and(nonempty_json_value)
    {
        return Err(gateway_error("inference_tool_call_not_allowed_part34_5"));
    }
    let text = choice
        .message
        .content
        .as_deref()
        .ok_or_else(|| gateway_error("inference_text_missing"))?;
    if text.is_empty() {
        return Err(gateway_error("inference_text_empty"));
    }
    if let Some(reason) = choice.finish_reason.as_deref() {
        validate_bounded_text(reason, MAX_FINISH_REASON_BYTES, "invalid_inference_finish_reason")?;
    }
    if let Some(model) = envelope.model.as_deref() {
        validate_bounded_text(model, MAX_MODEL_ID_BYTES, "invalid_inference_observed_model")?;
    }

    let usage = envelope.usage.as_ref().map(|value| TokenUsage {
        input: value.prompt_tokens.unwrap_or(0),
        output: value.completion_tokens.unwrap_or(0),
        cache_read_input: value
            .prompt_tokens_details
            .as_ref()
            .and_then(|details| details.cached_tokens),
    });
    let observed_model_id = envelope.model.clone();
    let finish_reason = choice.finish_reason.clone();

    Ok(GatewayChatResponse {
        requested_model_id: requested_model_id.to_owned(),
        observed_model_id,
        text: text.to_owned(),
        finish_reason,
        usage,
    })
}

fn nonempty_json_value(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Array(values) => !values.is_empty(),
        serde_json::Value::Object(values) => !values.is_empty(),
        _ => true,
    }
}

fn validate_bounded_text(value: &str, max_bytes: usize, code: &'static str) -> Result<()> {
    if value.is_empty()
        || value.len() > max_bytes
        || value.trim() != value
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(gateway_error(code));
    }
    Ok(())
}

fn gateway_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Gateway(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibecoder_domain::ModelRef;
    use vibecoder_gateway_contract::GatewayChatMessage;

    fn request() -> GatewayChatRequest {
        GatewayChatRequest {
            model: ModelRef {
                id: "provider/model".into(),
                display_name: None,
                provider: Some("provider".into()),
            },
            messages: vec![GatewayChatMessage {
                role: GatewayChatRole::User,
                content: "hello".into(),
            }],
            max_output_tokens: 128,
        }
    }

    fn response(body: &str) -> RawGatewayResponse {
        RawGatewayResponse {
            status: 200,
            content_type: Some("application/json; charset=utf-8".into()),
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn request_is_non_streaming_exact_model_and_bounded() {
        let req = request();
        validate_request(&req).unwrap();
        let wire = serde_json::to_value(build_wire_request(&req)).unwrap();
        assert_eq!(wire["model"], "provider/model");
        assert_eq!(wire["stream"], false);
        assert_eq!(wire["max_tokens"], 128);
        assert_eq!(wire["messages"][0]["role"], "user");
    }

    #[test]
    fn parses_text_and_usage_without_exposing_raw_server_error() {
        let parsed = interpret_chat_response(
            response(r#"{"model":"provider/model","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":7,"completion_tokens":2,"total_tokens":9,"prompt_tokens_details":{"cached_tokens":3}}}"#),
            GatewayCredential::Anonymous,
            "provider/model",
        )
        .unwrap();
        assert_eq!(parsed.text, "hi");
        assert_eq!(parsed.usage.unwrap().cache_read_input, Some(3));
    }

    #[test]
    fn tool_calls_are_rejected_before_part34_7() {
        let error = interpret_chat_response(
            response(r#"{"choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{"id":"x"}]},"finish_reason":"tool_calls"}]}"#),
            GatewayCredential::Anonymous,
            "provider/model",
        )
        .unwrap_err();
        assert!(matches!(error, VibeCoderError::Gateway(code) if code == "inference_tool_call_not_allowed_part34_5"));
    }

    #[test]
    fn status_errors_are_stable_and_do_not_parse_provider_prose() {
        let error = interpret_chat_response(
            RawGatewayResponse {
                status: 429,
                content_type: Some("application/json".into()),
                body: br#"{"error":{"message":"secret provider prose"}}"#.to_vec(),
            },
            GatewayCredential::Secret("redacted"),
            "provider/model",
        )
        .unwrap_err();
        assert!(matches!(error, VibeCoderError::Gateway(code) if code == "inference_rate_limited"));
    }
}
