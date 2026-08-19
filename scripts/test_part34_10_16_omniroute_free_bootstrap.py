#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def die(code):
    raise SystemExit('test_part34_10_16_omniroute_free_bootstrap: ' + code)

bootstrap = (ROOT / 'crates/vibecoder-gateway-omniroute/src/bootstrap.rs').read_text()
controller = (ROOT / 'crates/vibecoder-android-host/src/app_controller_ffi.rs').read_text()
lib = (ROOT / 'crates/vibecoder-gateway-omniroute/src/lib.rs').read_text()
profile = (ROOT / 'config/omniroute-android-runtime-profile.json').read_text()
lock = (ROOT / 'third_party/SOURCES.lock.json').read_text()

for token in (
    'VIBECODER_FREE_PROVIDER_ID: &str = "opencode"',
    'VIBECODER_FREE_PROVIDER_NAME: &str = "OpenCode Free"',
    'provider_bootstrap_non_loopback_forbidden',
    '.append_pair("provider", VIBECODER_FREE_PROVIDER_ID)',
    '"provider": VIBECODER_FREE_PROVIDER_ID',
    '"priority": 1',
    'Method::PATCH',
    '"isActive": true',
    'sync-models',
    '.append_pair("mode", "import")',
    'MAX_MANAGEMENT_RESPONSE_BYTES',
    '.redirect(reqwest::redirect::Policy::none())',
    '.no_proxy()',
):
    if token not in bootstrap:
        die('bootstrap_contract_missing:' + token)

# The no-auth provider must stay no-auth. Never smuggle a placeholder credential into the DB.
create_start = bootstrap.index('let body = serde_json::to_vec(&json!({')
create_end = bootstrap.index('}))', create_start)
create_body = bootstrap[create_start:create_end]
if 'apiKey' in create_body or 'api_key' in create_body:
    die('free_provider_create_must_not_contain_fake_secret')
if 'cloudflare-playground' in create_body or 'pollinations' in create_body:
    die('browser_or_unreviewed_default_provider_selected')

if 'mod bootstrap;' not in lib or 'OmniRouteProviderBootstrap' not in lib:
    die('bootstrap_module_not_exported')

bootstrap_fn = controller[controller.index('fn bootstrap_snapshot_bytes'):controller.index('fn select_exact_model')]
if bootstrap_fn.index('deterministic_profile(&profile)') > bootstrap_fn.index('load_models_with_free_provider_bootstrap'):
    die('provider_mutation_must_follow_runtime_profile_attestation')
if 'Err(VibeCoderError::Gateway(code)) if code == "no_usable_chat_models"' not in bootstrap_fn:
    die('automatic_provider_bootstrap_must_only_trigger_for_empty_catalog')
if 'FREE_PROVIDER_MODEL_POLL_ATTEMPTS: usize = 24' not in controller:
    die('provider_model_poll_must_be_bounded')
if 'tokio::time::sleep' not in bootstrap_fn:
    die('provider_catalog_poll_delay_missing')
if 'fallback' in bootstrap_fn.lower():
    die('provider_bootstrap_must_not_enable_model_fallback')

# Keep the reviewed Android profile deterministic and on the latest currently reviewed 3.8.50 rail.
for token in ('"version": "3.8.50"', '"required_node_version": "24.19.0"'):
    if token not in profile:
        die('runtime_profile_pin_missing:' + token)
if '"version": "3.8.50"' not in lock:
    die('omniroute_source_lock_not_3_8_50')

print('Part 34.10.16 OmniRoute automatic free-provider bootstrap regression PASSED')
