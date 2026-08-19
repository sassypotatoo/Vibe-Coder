use super::{
    AndroidAsyncExecutor, AndroidHostPaths, AndroidHostRuntime, ANDROID_HOST_FFI_ERROR,
    ANDROID_HOST_FFI_MAX_JSON_BYTES, ffi_path, host_error,
};
use crate::omniroute_service::LOOPBACK_BASE_URL;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use uuid::Uuid;
use vibecoder_agent_jcode::JcodeAgentRuntime;
use vibecoder_checkpoint_local::{LocalCheckpointConfig, LocalCheckpointStore};
use vibecoder_core::{ConversationModelTurnCancellation, VibeCoderCore};
use vibecoder_domain::{ConversationId, ModelRef, ProjectId, Result, VibeCoderError};
use vibecoder_gateway_contract::{GatewayCredential, GatewayExecutionProfile};
use vibecoder_gateway_omniroute::{OmniRouteClient, OmniRouteConfig, OmniRouteProviderBootstrap};
use vibecoder_persistence_local::{LocalProjectStateConfig, LocalProjectStateStore};
use vibecoder_routing::{ModelRoutePolicyConfig, ModelRouteTargetConfig};
use vibecoder_workspace_local::{LocalWorkspaceConfig, LocalWorkspaceRuntime};

const CHAT_MAX_PROMPT_BYTES: usize = 128 * 1024;
const CHAT_MAX_OUTPUT_TOKENS: u32 = 4096;
const CHAT_GATEWAY_TIMEOUT_MS: u64 = 120_000;
const CHAT_GATEWAY_MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const CHAT_FFI_MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const FREE_PROVIDER_MODEL_POLL_ATTEMPTS: usize = 24;
const FREE_PROVIDER_MODEL_POLL_DELAY_MS: u64 = 250;

static APP_CONTROLLER: OnceLock<Mutex<Option<Arc<AndroidAppController>>>> = OnceLock::new();

type AndroidCore = VibeCoderCore<JcodeAgentRuntime, OmniRouteClient, LocalWorkspaceRuntime>;

struct AndroidAppController {
    key: String,
    core: AndroidCore,
    executor: Arc<AndroidAsyncExecutor>,
    provider_bootstrap: OmniRouteProviderBootstrap,
    selected_model: Mutex<Option<ModelRef>>,
    active_turn: Mutex<Option<ActiveChatTurn>>,
}

#[derive(Clone)]
enum ActiveChatCancellation {
    Model(ConversationModelTurnCancellation),
    AgentAction,
}

#[derive(Clone)]
struct ActiveChatTurn {
    project_id: ProjectId,
    conversation_id: ConversationId,
    cancellation: ActiveChatCancellation,
}

struct ActiveTurnRegistration<'a> {
    slot: &'a Mutex<Option<ActiveChatTurn>>,
    project_id: ProjectId,
    conversation_id: ConversationId,
}

impl Drop for ActiveTurnRegistration<'_> {
    fn drop(&mut self) {
        match self.slot.lock() {
            Ok(mut active) => {
                if active.as_ref().is_some_and(|turn| {
                    turn.project_id == self.project_id && turn.conversation_id == self.conversation_id
                }) {
                    *active = None;
                }
            }
            Err(poisoned) => {
                let mut active = poisoned.into_inner();
                if active.as_ref().is_some_and(|turn| {
                    turn.project_id == self.project_id && turn.conversation_id == self.conversation_id
                }) {
                    *active = None;
                }
            }
        }
    }
}

#[derive(Serialize)]
struct BootstrapSnapshot {
    schema: u32,
    status: &'static str,
    controller_ready: bool,
    chat_ready: bool,
    runtime_profile_verified: bool,
    usable_models: usize,
    selected_model_id: Option<String>,
    provider_setup_required: bool,
    error: Option<String>,
}

#[derive(Serialize)]
struct CreateChatSnapshot {
    schema: u32,
    status: &'static str,
    project_id: String,
    conversation_id: String,
}

#[derive(Serialize)]
struct SendChatSnapshot {
    schema: u32,
    status: &'static str,
    turn_kind: &'static str,
    project_id: String,
    conversation_id: String,
    model_id: String,
    observed_model_id: Option<String>,
    finish_reason: Option<String>,
    assistant_text: String,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
    successful_file_tool_calls: Option<usize>,
    successful_mutation_tool_calls: Option<usize>,
    workspace_change_proven: Option<bool>,
}

#[derive(Serialize)]
struct CancelChatSnapshot {
    schema: u32,
    status: &'static str,
    cancel_requested: bool,
}

#[derive(Serialize)]
struct ErrorSnapshot {
    schema: u32,
    status: &'static str,
    error: String,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vibecoder_android_host_app_controller_init_json(
    app_private_dir: *const c_char,
    native_library_dir: *const c_char,
    packaged_executable_dir: *const c_char,
    inventory_json: *const u8,
    inventory_len: usize,
    output: *mut u8,
    output_capacity: usize,
) -> i64 {
    one_shot_json(output, output_capacity, || unsafe {
        controller_init_bytes(
            app_private_dir,
            native_library_dir,
            packaged_executable_dir,
            inventory_json,
            inventory_len,
        )
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vibecoder_android_host_chat_create_json(
    output: *mut u8,
    output_capacity: usize,
) -> i64 {
    one_shot_json(output, output_capacity, chat_create_bytes)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vibecoder_android_host_chat_send_json(
    project_id: *const c_char,
    conversation_id: *const c_char,
    prompt_utf8: *const u8,
    prompt_len: usize,
    output: *mut u8,
    output_capacity: usize,
) -> i64 {
    one_shot_json(output, output_capacity, || unsafe {
        chat_send_bytes(project_id, conversation_id, prompt_utf8, prompt_len)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vibecoder_android_host_chat_cancel_json(
    project_id: *const c_char,
    conversation_id: *const c_char,
    output: *mut u8,
    output_capacity: usize,
) -> i64 {
    one_shot_json(output, output_capacity, || unsafe {
        chat_cancel_bytes(project_id, conversation_id)
    })
}

fn one_shot_json<F>(output: *mut u8, output_capacity: usize, operation: F) -> i64
where
    F: FnOnce() -> Result<Vec<u8>>,
{
    if output.is_null() || output_capacity == 0 || output_capacity > CHAT_FFI_MAX_OUTPUT_BYTES {
        return ANDROID_HOST_FFI_ERROR;
    }
    let bytes = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(error)) => error_json(&error),
        Err(_) => serde_json::to_vec(&ErrorSnapshot {
            schema: 1,
            status: "failed",
            error: "android_app_controller_panic".into(),
        })
        .unwrap_or_else(|_| br#"{"schema":1,"status":"failed","error":"serialization_failed"}"#.to_vec()),
    };
    if bytes.is_empty() || bytes.len() > output_capacity || bytes.len() > CHAT_FFI_MAX_OUTPUT_BYTES {
        return ANDROID_HOST_FFI_ERROR;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len());
    }
    bytes.len() as i64
}

unsafe fn controller_init_bytes(
    app_private_dir: *const c_char,
    native_library_dir: *const c_char,
    packaged_executable_dir: *const c_char,
    inventory_json: *const u8,
    inventory_len: usize,
) -> Result<Vec<u8>> {
    if app_private_dir.is_null()
        || native_library_dir.is_null()
        || packaged_executable_dir.is_null()
        || inventory_json.is_null()
        || inventory_len == 0
        || inventory_len > ANDROID_HOST_FFI_MAX_JSON_BYTES
    {
        return Err(host_error("android_app_controller_arguments_invalid"));
    }
    let app = unsafe { ffi_path(app_private_dir)? };
    let native = unsafe { ffi_path(native_library_dir)? };
    let exec = unsafe { ffi_path(packaged_executable_dir)? };
    let inventory = unsafe { std::slice::from_raw_parts(inventory_json, inventory_len) };
    let paths = AndroidHostPaths::new(&app, &native, &exec)?;
    let key = controller_key(&paths, inventory);

    let slot = APP_CONTROLLER.get_or_init(|| Mutex::new(None));
    let controller = {
        let mut guard = slot
            .lock()
            .map_err(|_| host_error("android_app_controller_slot_poisoned"))?;
        if let Some(existing) = guard.as_ref() {
            if existing.key != key {
                return Err(host_error("android_app_controller_identity_mismatch"));
            }
            Arc::clone(existing)
        } else {
            let controller = Arc::new(AndroidAppController::initialize(key, paths, inventory)?);
            *guard = Some(Arc::clone(&controller));
            controller
        }
    };

    controller.bootstrap_snapshot_bytes()
}

fn chat_create_bytes() -> Result<Vec<u8>> {
    let controller = current_controller()?;
    let project = controller.executor.block_on(controller.core.create_persisted_project())?;
    let conversation_id = match controller
        .executor
        .block_on(controller.core.create_persisted_conversation(&project))
    {
        Ok(conversation_id) => conversation_id,
        Err(error) => {
            let _ = controller
                .executor
                .block_on(controller.core.remove_persisted_project(&project));
            return Err(error);
        }
    };
    serialize_bounded(&CreateChatSnapshot {
        schema: 1,
        status: "created",
        project_id: project.id.0.hyphenated().to_string(),
        conversation_id: conversation_id.0.hyphenated().to_string(),
    })
}

unsafe fn chat_send_bytes(
    project_id: *const c_char,
    conversation_id: *const c_char,
    prompt_utf8: *const u8,
    prompt_len: usize,
) -> Result<Vec<u8>> {
    if project_id.is_null()
        || conversation_id.is_null()
        || prompt_utf8.is_null()
        || prompt_len == 0
        || prompt_len > CHAT_MAX_PROMPT_BYTES
    {
        return Err(host_error("android_chat_send_arguments_invalid"));
    }
    let project_id = unsafe { parse_project_id_cstr(project_id)? };
    let conversation_id = unsafe { parse_conversation_id_cstr(conversation_id)? };
    let prompt_bytes = unsafe { std::slice::from_raw_parts(prompt_utf8, prompt_len) };
    let prompt = std::str::from_utf8(prompt_bytes)
        .map_err(|_| host_error("android_chat_prompt_not_utf8"))?;
    if prompt.trim().is_empty() || prompt.contains('\0') {
        return Err(host_error("android_chat_prompt_invalid"));
    }

    let controller = current_controller()?;
    let model = controller.select_exact_model()?;
    let route = classify_chat_route(prompt);
    let model_cancellation = if route == ChatRoute::ModelChat {
        Some(ConversationModelTurnCancellation::new())
    } else {
        None
    };
    let active_cancellation = match model_cancellation.as_ref() {
        Some(value) => ActiveChatCancellation::Model(value.clone()),
        None => ActiveChatCancellation::AgentAction,
    };
    {
        let mut active = controller
            .active_turn
            .lock()
            .map_err(|_| host_error("android_chat_active_turn_poisoned"))?;
        if active.is_some() {
            return Err(host_error("android_chat_turn_already_active"));
        }
        *active = Some(ActiveChatTurn {
            project_id,
            conversation_id,
            cancellation: active_cancellation,
        });
    }
    let _registration = ActiveTurnRegistration {
        slot: &controller.active_turn,
        project_id,
        conversation_id,
    };

    match route {
        ChatRoute::ModelChat => {
            let cancellation = model_cancellation
                .as_ref()
                .ok_or_else(|| host_error("android_chat_model_cancellation_missing"))?;
            let outcome = controller.executor.block_on(
                controller.core.run_persisted_model_conversation_turn_cancellable(
                    project_id,
                    conversation_id,
                    GatewayCredential::Anonymous,
                    &model.id,
                    CHAT_MAX_OUTPUT_TOKENS,
                    prompt,
                    cancellation,
                ),
            )?;
            let usage = outcome.usage();
            serialize_bounded(&SendChatSnapshot {
                schema: 1,
                status: "completed",
                turn_kind: "model_chat",
                project_id: project_id.0.hyphenated().to_string(),
                conversation_id: conversation_id.0.hyphenated().to_string(),
                model_id: outcome.model().id.clone(),
                observed_model_id: outcome.observed_model_id().map(str::to_owned),
                finish_reason: outcome.finish_reason().map(str::to_owned),
                assistant_text: outcome.assistant_text().to_owned(),
                input_tokens: usage.map(|value| value.input),
                output_tokens: usage.map(|value| value.output),
                cached_input_tokens: usage.and_then(|value| value.cache_read_input),
                successful_file_tool_calls: None,
                successful_mutation_tool_calls: None,
                workspace_change_proven: None,
            })
        }
        ChatRoute::AgentAction => {
            let policy = exact_model_route_policy(&model);
            let outcome = controller.executor.block_on(
                controller.core.run_persisted_agent_action_turn(
                    project_id,
                    conversation_id,
                    GatewayCredential::Anonymous,
                    &policy,
                    prompt,
                    None,
                ),
            )?;
            let backend = outcome.backend();
            let turn = backend.turn();
            let usage = turn.usage;
            serialize_bounded(&SendChatSnapshot {
                schema: 1,
                status: "completed",
                turn_kind: "agent_action",
                project_id: project_id.0.hyphenated().to_string(),
                conversation_id: conversation_id.0.hyphenated().to_string(),
                model_id: backend.model().id.clone(),
                observed_model_id: None,
                finish_reason: None,
                assistant_text: turn.text.clone(),
                input_tokens: usage.map(|value| value.input),
                output_tokens: usage.map(|value| value.output),
                cached_input_tokens: usage.and_then(|value| value.cache_read_input),
                successful_file_tool_calls: Some(outcome.successful_file_tool_calls()),
                successful_mutation_tool_calls: Some(outcome.successful_mutation_tool_calls()),
                workspace_change_proven: Some(outcome.workspace_change_proven()),
            })
        }
    }
}

unsafe fn chat_cancel_bytes(
    project_id: *const c_char,
    conversation_id: *const c_char,
) -> Result<Vec<u8>> {
    if project_id.is_null() || conversation_id.is_null() {
        return Err(host_error("android_chat_cancel_arguments_invalid"));
    }
    let project_id = unsafe { parse_project_id_cstr(project_id)? };
    let conversation_id = unsafe { parse_conversation_id_cstr(conversation_id)? };
    let controller = current_controller()?;
    let cancellation = {
        let active = controller
            .active_turn
            .lock()
            .map_err(|_| host_error("android_chat_active_turn_poisoned"))?;
        active.as_ref().and_then(|turn| {
            (turn.project_id == project_id && turn.conversation_id == conversation_id)
                .then(|| turn.cancellation.clone())
        })
    };
    let requested = match cancellation {
        Some(ActiveChatCancellation::Model(cancellation)) => {
            cancellation.request();
            true
        }
        Some(ActiveChatCancellation::AgentAction) => {
            controller.executor.block_on(
                controller
                    .core
                    .cancel_persisted_conversation_turn(project_id, conversation_id),
            )?;
            true
        }
        None => false,
    };
    serialize_bounded(&CancelChatSnapshot {
        schema: 1,
        status: if requested { "cancel_requested" } else { "no_matching_active_turn" },
        cancel_requested: requested,
    })
}

impl AndroidAppController {
    fn initialize(key: String, paths: AndroidHostPaths, inventory: &[u8]) -> Result<Self> {
        let app_private_dir = paths.app_private_dir().to_path_buf();
        let host = AndroidHostRuntime::from_inventory_json(paths, inventory)?;
        let executor = host.async_executor();
        let agent = JcodeAgentRuntime::new(host.jcode_connection_config()?)?;
        let gateway_config = OmniRouteConfig {
            base_url: LOOPBACK_BASE_URL.into(),
            request_timeout_ms: CHAT_GATEWAY_TIMEOUT_MS,
            max_response_bytes: CHAT_GATEWAY_MAX_RESPONSE_BYTES,
        };
        let provider_bootstrap = OmniRouteProviderBootstrap::new(gateway_config.clone())?;
        let gateway = OmniRouteClient::new(gateway_config)?;
        let workspace = LocalWorkspaceRuntime::initialize(LocalWorkspaceConfig {
            app_private_dir: app_private_dir.clone(),
        })?;
        let store = Arc::new(LocalProjectStateStore::initialize(LocalProjectStateConfig {
            app_private_dir: app_private_dir.clone(),
        })?);
        let checkpoint_store = Arc::new(LocalCheckpointStore::initialize(LocalCheckpointConfig {
            app_private_dir,
        })?);
        let core = VibeCoderCore::new(agent, gateway, workspace)
            .with_project_state_store(store.clone())
            .with_conversation_store(store)
            .with_checkpoint_store(checkpoint_store);
        Ok(Self {
            key,
            core,
            executor,
            provider_bootstrap,
            selected_model: Mutex::new(None),
            active_turn: Mutex::new(None),
        })
    }

    fn bootstrap_snapshot_bytes(&self) -> Result<Vec<u8>> {
        let result = self.executor.block_on(async {
            let profile = self
                .core
                .gateway_execution_profile(GatewayCredential::Anonymous)
                .await?;
            if !deterministic_profile(&profile) {
                return Err(host_error("android_chat_gateway_profile_not_deterministic"));
            }
            let models = self.load_models_with_free_provider_bootstrap().await;
            Ok::<_, VibeCoderError>((profile, models))
        });
        match result {
            Ok((_profile, Ok(mut models))) => {
                models.retain(|model| android_chat_model_id_usable(&model.id));
                models.sort_by(|left, right| left.id.cmp(&right.id));
                let selected = models.first().cloned();
                *self
                    .selected_model
                    .lock()
                    .map_err(|_| host_error("android_chat_model_selection_poisoned"))? = selected.clone();
                serialize_bounded(&BootstrapSnapshot {
                    schema: 1,
                    status: if selected.is_some() { "ready" } else { "provider_setup_required" },
                    controller_ready: true,
                    chat_ready: selected.is_some(),
                    runtime_profile_verified: true,
                    usable_models: models.len(),
                    selected_model_id: selected.map(|model| model.id),
                    provider_setup_required: models.is_empty(),
                    error: None,
                })
            }
            Ok((_profile, Err(error))) => {
                *self
                    .selected_model
                    .lock()
                    .map_err(|_| host_error("android_chat_model_selection_poisoned"))? = None;
                serialize_bounded(&BootstrapSnapshot {
                    schema: 1,
                    status: "gateway_not_ready",
                    controller_ready: true,
                    chat_ready: false,
                    runtime_profile_verified: true,
                    usable_models: 0,
                    selected_model_id: None,
                    provider_setup_required: false,
                    error: Some(stable_error_code(&error)),
                })
            }
            Err(error) => {
                *self
                    .selected_model
                    .lock()
                    .map_err(|_| host_error("android_chat_model_selection_poisoned"))? = None;
                serialize_bounded(&BootstrapSnapshot {
                    schema: 1,
                    status: "gateway_not_ready",
                    controller_ready: true,
                    chat_ready: false,
                    runtime_profile_verified: false,
                    usable_models: 0,
                    selected_model_id: None,
                    provider_setup_required: false,
                    error: Some(stable_error_code(&error)),
                })
            }
        }
    }

    async fn load_models_with_free_provider_bootstrap(&self) -> Result<Vec<ModelRef>> {
        match self
            .core
            .list_gateway_models(GatewayCredential::Anonymous)
            .await
        {
            Ok(models) => return Ok(models),
            Err(VibeCoderError::Gateway(code)) if code == "no_usable_chat_models" => {}
            Err(error) => return Err(error),
        }

        self.provider_bootstrap
            .ensure_vibecoder_free_provider()
            .await?;

        for attempt in 0..FREE_PROVIDER_MODEL_POLL_ATTEMPTS {
            match self
                .core
                .list_gateway_models(GatewayCredential::Anonymous)
                .await
            {
                Ok(models) => return Ok(models),
                Err(VibeCoderError::Gateway(code)) if code == "no_usable_chat_models" => {
                    if attempt + 1 == FREE_PROVIDER_MODEL_POLL_ATTEMPTS {
                        return Err(VibeCoderError::Gateway(code));
                    }
                    tokio::time::sleep(Duration::from_millis(FREE_PROVIDER_MODEL_POLL_DELAY_MS)).await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(host_error("android_chat_free_provider_catalog_poll_exhausted"))
    }

    fn select_exact_model(&self) -> Result<ModelRef> {
        // Bootstrap owns deterministic model selection. The core turn itself performs the fresh
        // profile/catalog verification inside its cancellable inference future, so doing another
        // network catalog call here would create an uncancellable Stop gap before the real turn.
        self.selected_model
            .lock()
            .map_err(|_| host_error("android_chat_model_selection_poisoned"))?
            .clone()
            .ok_or_else(|| host_error("android_chat_no_usable_models"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChatRoute {
    ModelChat,
    AgentAction,
}

fn exact_model_route_policy(model: &ModelRef) -> ModelRoutePolicyConfig {
    ModelRoutePolicyConfig {
        primary: ModelRouteTargetConfig {
            model_id: model.id.clone(),
            provider: model.provider.clone(),
        },
        fallbacks: Vec::new(),
        fallback_on: Vec::new(),
        fallback_boundary: Default::default(),
    }
}

fn classify_chat_route(prompt: &str) -> ChatRoute {
    // Keep automatic mutation routing conservative. A coding/project target and an action verb are
    // both required. Explanation-style questions remain normal model chat unless the user clearly
    // addresses the assistant with a direct request. This does not enable the explicit outer loop.
    let normalized = prompt
        .chars()
        .flat_map(char::to_lowercase)
        .map(|ch| if ch.is_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let padded = format!(" {normalized} ");
    let has_word = |word: &str| padded.contains(&format!(" {word} "));
    let has_phrase = |phrase: &str| padded.contains(&format!(" {phrase} "));

    let target = [
        "app", "project", "code", "file", "button", "screen", "ui", "layout", "page",
        "component", "feature", "function", "class", "method", "bug", "error", "android",
        "gradle", "manifest", "xml", "html", "css", "javascript", "typescript", "kotlin",
        "java", "rust", "dependency", "route", "api", "endpoint", "test", "tests",
    ]
    .iter()
    .any(|word| has_word(word));
    if !target {
        return ChatRoute::ModelChat;
    }

    let action = [
        "add", "create", "make", "implement", "build", "fix", "change", "update", "remove",
        "delete", "rename", "edit", "modify", "refactor", "wire", "connect", "replace",
        "insert", "generate", "design", "bana", "banao", "bnado", "bna", "karo", "kardo",
        "lagao", "jodo", "hatao", "theek", "thik",
    ]
    .iter()
    .any(|word| has_word(word));
    if !action {
        return ChatRoute::ModelChat;
    }

    let direct_request = [
        "please", "can you", "could you", "would you", "i want you to", "bana do", "bana de",
        "bna do", "bna de", "kar do", "kar de", "kardo", "laga do", "jod do", "hata do",
    ]
    .iter()
    .any(|phrase| has_phrase(phrase));
    let explanation_prefix = [
        "how ", "how to ", "how do i ", "how can i ", "why ", "what ", "what is ",
        "where ", "when ", "which ", "who ", "can i ", "should i ", "kya ", "kaise ",
        "kyu ", "kyun ", "explain ", "tell me ", "samjha ",
    ]
    .iter()
    .any(|prefix| normalized.starts_with(prefix));

    if explanation_prefix && !direct_request {
        ChatRoute::ModelChat
    } else {
        ChatRoute::AgentAction
    }
}

fn android_chat_model_id_usable(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value.trim() == value
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn current_controller() -> Result<Arc<AndroidAppController>> {
    let slot = APP_CONTROLLER.get_or_init(|| Mutex::new(None));
    let guard = slot
        .lock()
        .map_err(|_| host_error("android_app_controller_slot_poisoned"))?;
    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| host_error("android_app_controller_not_initialized"))
}

fn deterministic_profile(profile: &GatewayExecutionProfile) -> bool {
    !profile.gateway_id.is_empty()
        && !profile.profile_id.is_empty()
        && profile.profile_sha256.len() == 64
        && profile.permits_exact_model_execution()
}

fn controller_key(paths: &AndroidHostPaths, inventory: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(inventory);
    let inventory_sha = format!("{:x}", digest.finalize());
    format!(
        "{}|{}|{}|{}",
        paths.app_private_dir().display(),
        paths.native_library_dir().display(),
        paths.packaged_executable_dir().display(),
        inventory_sha,
    )
}

unsafe fn parse_project_id_cstr(pointer: *const c_char) -> Result<ProjectId> {
    let text = unsafe { bounded_cstr(pointer, 64, "android_chat_project_id_invalid")? };
    let uuid = Uuid::parse_str(&text).map_err(|_| host_error("android_chat_project_id_invalid"))?;
    if uuid.hyphenated().to_string() != text {
        return Err(host_error("android_chat_project_id_invalid"));
    }
    Ok(ProjectId(uuid))
}

unsafe fn parse_conversation_id_cstr(pointer: *const c_char) -> Result<ConversationId> {
    let text = unsafe { bounded_cstr(pointer, 64, "android_chat_conversation_id_invalid")? };
    let uuid = Uuid::parse_str(&text)
        .map_err(|_| host_error("android_chat_conversation_id_invalid"))?;
    if uuid.hyphenated().to_string() != text {
        return Err(host_error("android_chat_conversation_id_invalid"));
    }
    Ok(ConversationId(uuid))
}

unsafe fn bounded_cstr(
    pointer: *const c_char,
    max_bytes: usize,
    code: &'static str,
) -> Result<String> {
    if pointer.is_null() {
        return Err(host_error(code));
    }
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    if bytes.is_empty() || bytes.len() > max_bytes {
        return Err(host_error(code));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| host_error(code))?;
    if text.trim() != text || text.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(host_error(code));
    }
    Ok(text.to_owned())
}

fn serialize_bounded<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| host_error("android_app_controller_json_failed"))?;
    if bytes.is_empty() || bytes.len() > CHAT_FFI_MAX_OUTPUT_BYTES {
        return Err(host_error("android_app_controller_json_too_large"));
    }
    Ok(bytes)
}

fn error_json(error: &VibeCoderError) -> Vec<u8> {
    serde_json::to_vec(&ErrorSnapshot {
        schema: 1,
        status: if matches!(error, VibeCoderError::Cancelled) {
            "cancelled"
        } else {
            "failed"
        },
        error: stable_error_code(error),
    })
    .unwrap_or_else(|_| br#"{"schema":1,"status":"failed","error":"serialization_failed"}"#.to_vec())
}

fn stable_error_code(error: &VibeCoderError) -> String {
    let raw = match error {
        VibeCoderError::InvalidRequest(code)
        | VibeCoderError::Agent(code)
        | VibeCoderError::Gateway(code)
        | VibeCoderError::Routing(code)
        | VibeCoderError::Config(code)
        | VibeCoderError::Secret(code)
        | VibeCoderError::Workspace(code)
        | VibeCoderError::Command(code)
        | VibeCoderError::Process(code)
        | VibeCoderError::Persistence(code)
        | VibeCoderError::Checkpoint(code)
        | VibeCoderError::Build(code) => code.as_str(),
        VibeCoderError::MissingCapability { capability, .. } => capability,
        VibeCoderError::Cancelled => "cancelled",
    };
    let mut out = String::with_capacity(raw.len().min(160));
    for byte in raw.bytes().take(160) {
        out.push(if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.') {
            byte as char
        } else {
            '_'
        });
    }
    if out.is_empty() { "unknown_error".into() } else { out }
}


#[cfg(test)]
mod app_controller_route_tests {
    use super::{classify_chat_route, ChatRoute};

    #[test]
    fn routes_clear_coding_mutations_to_agent_action() {
        for prompt in [
            "Add a Start button to the home screen",
            "Please fix this Gradle error",
            "Home screen pe Start button bana do",
            "Connect the API endpoint to this page",
        ] {
            assert_eq!(classify_chat_route(prompt), ChatRoute::AgentAction, "{prompt}");
        }
    }

    #[test]
    fn keeps_questions_and_non_coding_chat_on_model_path() {
        for prompt in [
            "How do I add a button in Android?",
            "Why is this Gradle error happening?",
            "What is Rust ownership?",
            "Tell me a joke",
        ] {
            assert_eq!(classify_chat_route(prompt), ChatRoute::ModelChat, "{prompt}");
        }
    }

    #[test]
    fn direct_question_form_can_still_request_an_action() {
        assert_eq!(
            classify_chat_route("Can you add a Start button to the app?"),
            ChatRoute::AgentAction
        );
    }
}
