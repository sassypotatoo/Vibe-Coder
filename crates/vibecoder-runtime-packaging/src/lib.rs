//! Android ARM64 runtime-package inventory and proof boundary for Part 26.
//!
//! Android 10+ forbids treating writable app-home files as executable code. This crate therefore
//! separates package-installed/native code from writable runtime data and refuses to call a
//! component ready until the evidence required by its artifact class has been supplied.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use vibecoder_domain::{Result, VibeCoderError};

mod native_probe;
pub use native_probe::{
    NativeArtifactProbe, probe_android_native_artifact, probe_android_native_executable,
    version_requirement_is_supported,
};

pub const INVENTORY_SCHEMA: u32 = 1;
pub const ANDROID_ARM64_ABI: &str = "arm64-v8a";
pub const ANDROID_ARM64_RUST_TARGET: &str = "aarch64-linux-android";
pub const ANDROID_WX_TARGET_API: u32 = 29;
pub const REQUIRED_LARGE_PAGE_BYTES: u64 = 16 * 1024;
const MAX_COMPONENTS: usize = 64;
const MAX_COMPONENT_ID_BYTES: usize = 64;
const MAX_VERSION_REQUIREMENT_BYTES: usize = 128;
const MAX_RELATIVE_PATH_BYTES: usize = 4096;
const MAX_PATH_COMPONENT_BYTES: usize = 255;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCapability {
    Core,
    Agent,
    Gateway,
    WebsiteBuild,
    AndroidBuild,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeArtifactKind {
    InProcessNative,
    NativeExecutable,
    NativeLibrary,
    DataBundle,
    JavaArchive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePlacement {
    /// Native code linked/loaded from the installed APK/AAB native-library payload.
    ApkNativeLibrary,
    /// A package-installed native file that the Android adapter intends to invoke as a process.
    ApkNativeExecutable,
    /// Native executable delivered by a Google Play on-demand feature split. It remains package
    /// installed code and must never be copied into writable app data for execution.
    PlayFeatureNativeExecutable,
    /// Native executable delivered as a signed Android package split outside Google Play. The
    /// package manager installs it into a package-owned executable location; writable app data is
    /// never used as an execution root.
    PackageSplitNativeExecutable,
    /// Non-executable bytes shipped with the app and optionally materialized as data.
    ApkAsset,
    /// Writable app-private data. Never valid for native code or a process executable.
    WritableAppData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeComponentSpec {
    pub component_id: String,
    pub version_requirement: String,
    /// True only after the required runtime identity/constraint has been deliberately pinned.
    #[serde(default)]
    pub version_requirement_pinned: bool,
    pub artifact_kind: RuntimeArtifactKind,
    pub placement: RuntimePlacement,
    #[serde(default)]
    pub delivery_module: Option<String>,
    #[serde(default)]
    pub bundled_in_base: Option<bool>,
    #[serde(default)]
    pub relative_path: Option<PathBuf>,
    pub required_for: Vec<RuntimeCapability>,
    #[serde(default)]
    pub requires_exec_probe: bool,
    #[serde(default)]
    pub requires_version_probe: bool,
    #[serde(default)]
    pub requires_unix_socket_probe: bool,
    #[serde(default)]
    pub requires_service_probe: bool,
    #[serde(default)]
    pub requires_runtime_binding_probe: bool,
    #[serde(default)]
    pub requires_16k_page_compatibility: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidArm64RuntimeInventory {
    pub schema: u32,
    pub target_os: String,
    pub abi: String,
    pub rust_target: String,
    pub writable_app_home_exec_forbidden_from_target_api: u32,
    pub components: Vec<RuntimeComponentSpec>,
}

impl AndroidArm64RuntimeInventory {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let inventory: Self = serde_json::from_slice(bytes)
            .map_err(|_| packaging_error("runtime_inventory_json_invalid"))?;
        inventory.validate()?;
        Ok(inventory)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != INVENTORY_SCHEMA {
            return Err(packaging_error("runtime_inventory_schema_unsupported"));
        }
        if self.target_os != "android"
            || self.abi != ANDROID_ARM64_ABI
            || self.rust_target != ANDROID_ARM64_RUST_TARGET
        {
            return Err(packaging_error("runtime_inventory_target_mismatch"));
        }
        if self.writable_app_home_exec_forbidden_from_target_api != ANDROID_WX_TARGET_API {
            return Err(packaging_error("runtime_inventory_wx_policy_mismatch"));
        }
        if self.components.is_empty() || self.components.len() > MAX_COMPONENTS {
            return Err(packaging_error("runtime_inventory_component_count_invalid"));
        }

        let mut ids = HashSet::new();
        for component in &self.components {
            validate_component(component)?;
            if !ids.insert(component.component_id.as_str()) {
                return Err(packaging_error("runtime_inventory_component_duplicate"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeState {
    Passed,
    Failed,
    NotRun,
}

impl Default for ProbeState {
    fn default() -> Self {
        Self::NotRun
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeComponentEvidence {
    pub component_id: String,
    #[serde(default)]
    pub package_presence: ProbeState,
    #[serde(default)]
    pub arm64_identity: ProbeState,
    #[serde(default)]
    pub execution: ProbeState,
    #[serde(default)]
    pub version: ProbeState,
    #[serde(default)]
    pub unix_socket_round_trip: ProbeState,
    #[serde(default)]
    pub service_round_trip: ProbeState,
    #[serde(default)]
    pub runtime_binding: ProbeState,
    #[serde(default)]
    pub page_size_16k_compatibility: ProbeState,
    /// Requirement supplied by the signed/runtime inventory for actionable diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_version_requirement: Option<String>,
    /// Parsed semantic version actually observed from the trusted executable's version output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadinessBlocker {
    pub component_id: String,
    pub code: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AndroidRuntimeReadinessReport {
    pub core_ready: bool,
    pub agent_ready: bool,
    pub gateway_ready: bool,
    pub website_build_ready: bool,
    pub android_build_ready: bool,
    pub blockers: Vec<ReadinessBlocker>,
}

impl AndroidRuntimeReadinessReport {
    pub fn backend_ready(&self) -> bool {
        self.core_ready && self.agent_ready && self.gateway_ready
    }

    pub fn fully_ready(&self) -> bool {
        self.backend_ready() && self.website_build_ready && self.android_build_ready
    }
}

/// Evaluate proof supplied by the Android packaging/device adapter. `NotRun` is always fail-closed;
/// a manifest entry never becomes ready merely because its source contract exists.
pub fn evaluate_android_arm64_readiness(
    inventory: &AndroidArm64RuntimeInventory,
    evidence: &[RuntimeComponentEvidence],
) -> Result<AndroidRuntimeReadinessReport> {
    inventory.validate()?;

    let mut by_id = HashMap::new();
    for proof in evidence {
        validate_component_id(&proof.component_id)?;
        if by_id.insert(proof.component_id.as_str(), proof).is_some() {
            return Err(packaging_error("runtime_evidence_component_duplicate"));
        }
    }
    if by_id
        .keys()
        .any(|id| !inventory.components.iter().any(|component| component.component_id.as_str() == *id))
    {
        return Err(packaging_error("runtime_evidence_unknown_component"));
    }

    let mut blockers = Vec::new();
    let mut capability_ready = HashMap::from([
        (RuntimeCapability::Core, true),
        (RuntimeCapability::Agent, true),
        (RuntimeCapability::Gateway, true),
        (RuntimeCapability::WebsiteBuild, true),
        (RuntimeCapability::AndroidBuild, true),
    ]);
    let mut capability_present = HashSet::new();

    for component in &inventory.components {
        let proof = by_id.get(component.component_id.as_str()).copied();
        let mut ready = true;
        if !component.version_requirement_pinned {
            ready = false;
            blockers.push(ReadinessBlocker {
                component_id: component.component_id.clone(),
                code: "runtime_version_requirement_unpinned",
            });
        }
        require_probe(
            proof.map(|item| item.package_presence),
            &mut ready,
            &mut blockers,
            &component.component_id,
            "runtime_package_presence_unproven",
        );

        if matches!(
            component.artifact_kind,
            RuntimeArtifactKind::InProcessNative
                | RuntimeArtifactKind::NativeExecutable
                | RuntimeArtifactKind::NativeLibrary
        ) {
            require_probe(
                proof.map(|item| item.arm64_identity),
                &mut ready,
                &mut blockers,
                &component.component_id,
                "runtime_arm64_identity_unproven",
            );
        }
        if component.requires_exec_probe {
            require_probe(
                proof.map(|item| item.execution),
                &mut ready,
                &mut blockers,
                &component.component_id,
                "runtime_execution_unproven",
            );
        }
        if component.requires_version_probe {
            require_probe(
                proof.map(|item| item.version),
                &mut ready,
                &mut blockers,
                &component.component_id,
                "runtime_version_unproven",
            );
        }
        if component.requires_unix_socket_probe {
            require_probe(
                proof.map(|item| item.unix_socket_round_trip),
                &mut ready,
                &mut blockers,
                &component.component_id,
                "runtime_unix_socket_unproven",
            );
        }
        if component.requires_service_probe {
            require_probe(
                proof.map(|item| item.service_round_trip),
                &mut ready,
                &mut blockers,
                &component.component_id,
                "runtime_service_round_trip_unproven",
            );
        }
        if component.requires_runtime_binding_probe {
            require_probe(
                proof.map(|item| item.runtime_binding),
                &mut ready,
                &mut blockers,
                &component.component_id,
                "runtime_binding_unproven",
            );
        }
        if component.requires_16k_page_compatibility {
            require_probe(
                proof.map(|item| item.page_size_16k_compatibility),
                &mut ready,
                &mut blockers,
                &component.component_id,
                "runtime_16k_page_compatibility_unproven",
            );
        }

        for capability in &component.required_for {
            capability_present.insert(*capability);
            if !ready {
                capability_ready.insert(*capability, false);
            }
        }
    }

    for capability in capability_ready.keys().copied().collect::<Vec<_>>() {
        if !capability_present.contains(&capability) {
            capability_ready.insert(capability, false);
        }
    }

    Ok(AndroidRuntimeReadinessReport {
        core_ready: capability_ready[&RuntimeCapability::Core],
        agent_ready: capability_ready[&RuntimeCapability::Agent],
        gateway_ready: capability_ready[&RuntimeCapability::Gateway],
        website_build_ready: capability_ready[&RuntimeCapability::WebsiteBuild],
        android_build_ready: capability_ready[&RuntimeCapability::AndroidBuild],
        blockers,
    })
}

fn require_probe(
    state: Option<ProbeState>,
    ready: &mut bool,
    blockers: &mut Vec<ReadinessBlocker>,
    component_id: &str,
    code: &'static str,
) {
    if state != Some(ProbeState::Passed) {
        *ready = false;
        blockers.push(ReadinessBlocker {
            component_id: component_id.to_owned(),
            code,
        });
    }
}

fn validate_component(component: &RuntimeComponentSpec) -> Result<()> {
    validate_component_id(&component.component_id)?;
    let version = component.version_requirement.trim();
    if version.is_empty() || version.len() > MAX_VERSION_REQUIREMENT_BYTES {
        return Err(packaging_error("runtime_component_version_invalid"));
    }
    if component.required_for.is_empty() {
        return Err(packaging_error("runtime_component_capability_empty"));
    }
    let mut capabilities = HashSet::new();
    if component.required_for.iter().any(|capability| !capabilities.insert(*capability)) {
        return Err(packaging_error("runtime_component_capability_duplicate"));
    }

    if let Some(path) = &component.relative_path {
        validate_relative_path(path)?;
    }
    match component.artifact_kind {
        RuntimeArtifactKind::InProcessNative | RuntimeArtifactKind::NativeLibrary => {
            if component.placement != RuntimePlacement::ApkNativeLibrary
                || component.requires_exec_probe
                || !component.requires_16k_page_compatibility
            {
                return Err(packaging_error("runtime_native_library_placement_invalid"));
            }
        }
        RuntimeArtifactKind::NativeExecutable => {
            if !matches!(component.placement, RuntimePlacement::ApkNativeExecutable | RuntimePlacement::PlayFeatureNativeExecutable | RuntimePlacement::PackageSplitNativeExecutable)
                || component.relative_path.is_none()
                || !component.requires_exec_probe
                || !component.requires_16k_page_compatibility
            {
                return Err(packaging_error("runtime_native_executable_placement_invalid"));
            }
            if matches!(component.placement, RuntimePlacement::PlayFeatureNativeExecutable | RuntimePlacement::PackageSplitNativeExecutable) {
                let module = component.delivery_module.as_deref().unwrap_or("");
                if module.is_empty() || module.len() > 128 || component.bundled_in_base != Some(false) {
                    return Err(packaging_error(if component.placement == RuntimePlacement::PlayFeatureNativeExecutable {
                        "runtime_play_feature_delivery_invalid"
                    } else {
                        "runtime_package_split_delivery_invalid"
                    }));
                }
            }
        }
        RuntimeArtifactKind::DataBundle | RuntimeArtifactKind::JavaArchive => {
            if !matches!(
                component.placement,
                RuntimePlacement::ApkAsset | RuntimePlacement::WritableAppData
            ) || component.requires_exec_probe
                || component.requires_16k_page_compatibility
            {
                return Err(packaging_error("runtime_data_placement_invalid"));
            }
        }
    }

    if component.requires_unix_socket_probe
        && component.artifact_kind != RuntimeArtifactKind::NativeExecutable
    {
        return Err(packaging_error("runtime_socket_probe_kind_invalid"));
    }
    if component.requires_service_probe
        && !matches!(
            component.artifact_kind,
            RuntimeArtifactKind::NativeExecutable
                | RuntimeArtifactKind::DataBundle
                | RuntimeArtifactKind::JavaArchive
        )
    {
        return Err(packaging_error("runtime_service_probe_kind_invalid"));
    }
    if component.requires_runtime_binding_probe
        && !matches!(
            component.artifact_kind,
            RuntimeArtifactKind::NativeExecutable
                | RuntimeArtifactKind::DataBundle
                | RuntimeArtifactKind::JavaArchive
        )
    {
        return Err(packaging_error("runtime_binding_probe_kind_invalid"));
    }
    if component.version_requirement_pinned
        && component.requires_version_probe
        && !version_requirement_is_supported(&component.version_requirement)
    {
        return Err(packaging_error("runtime_component_version_requirement_unsupported"));
    }
    Ok(())
}

fn validate_component_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_COMPONENT_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(packaging_error("runtime_component_id_invalid"));
    }
    Ok(())
}

fn validate_relative_path(path: &Path) -> Result<()> {
    if path.is_absolute() {
        return Err(packaging_error("runtime_component_path_absolute"));
    }
    let text = path
        .to_str()
        .ok_or_else(|| packaging_error("runtime_component_path_non_utf8"))?;
    if text.is_empty()
        || text.len() > MAX_RELATIVE_PATH_BYTES
        || text.contains('\\')
        || text.chars().any(is_forbidden_display_char)
    {
        return Err(packaging_error("runtime_component_path_invalid"));
    }
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                let value = value
                    .to_str()
                    .ok_or_else(|| packaging_error("runtime_component_path_non_utf8"))?;
                if value.is_empty() || value.len() > MAX_PATH_COMPONENT_BYTES {
                    return Err(packaging_error("runtime_component_path_invalid"));
                }
            }
            Component::CurDir | Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(packaging_error("runtime_component_path_invalid"));
            }
        }
    }
    Ok(())
}

fn is_forbidden_display_char(ch: char) -> bool {
    ch.is_control()
        || matches!(
            ch,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

fn packaging_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Config(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn baseline() -> AndroidArm64RuntimeInventory {
        AndroidArm64RuntimeInventory {
            schema: INVENTORY_SCHEMA,
            target_os: "android".into(),
            abi: ANDROID_ARM64_ABI.into(),
            rust_target: ANDROID_ARM64_RUST_TARGET.into(),
            writable_app_home_exec_forbidden_from_target_api: ANDROID_WX_TARGET_API,
            components: vec![
                RuntimeComponentSpec {
                    component_id: "vibecoder_core".into(),
                    version_requirement: "0.1.0".into(),
                    version_requirement_pinned: true,
                    artifact_kind: RuntimeArtifactKind::InProcessNative,
                    placement: RuntimePlacement::ApkNativeLibrary,
                    delivery_module: None,
                    bundled_in_base: None,
                    relative_path: Some("libvibecoder_android_host.so".into()),
                    required_for: vec![RuntimeCapability::Core],
                    requires_exec_probe: false,
                    requires_version_probe: false,
                    requires_unix_socket_probe: false,
                    requires_service_probe: false,
                    requires_runtime_binding_probe: false,
                    requires_16k_page_compatibility: true,
                },
                RuntimeComponentSpec {
                    component_id: "jcode".into(),
                    version_requirement: "0.73.0".into(),
                    version_requirement_pinned: true,
                    artifact_kind: RuntimeArtifactKind::NativeExecutable,
                    placement: RuntimePlacement::ApkNativeExecutable,
                    delivery_module: None,
                    bundled_in_base: None,
                    relative_path: Some("libvibecoder_jcode_exec.so".into()),
                    required_for: vec![RuntimeCapability::Agent],
                    requires_exec_probe: true,
                    requires_version_probe: true,
                    requires_unix_socket_probe: true,
                    requires_service_probe: false,
                    requires_runtime_binding_probe: false,
                    requires_16k_page_compatibility: true,
                },
                RuntimeComponentSpec {
                    component_id: "node".into(),
                    version_requirement: ">=22.22.2 <23 || >=24.0.0 <27".into(),
                    version_requirement_pinned: true,
                    artifact_kind: RuntimeArtifactKind::NativeExecutable,
                    placement: RuntimePlacement::PlayFeatureNativeExecutable,
                    delivery_module: Some("node_runtime".into()),
                    bundled_in_base: Some(false),
                    relative_path: Some("libvibecoder_node_exec.so".into()),
                    required_for: vec![RuntimeCapability::Gateway, RuntimeCapability::WebsiteBuild],
                    requires_exec_probe: true,
                    requires_version_probe: true,
                    requires_unix_socket_probe: false,
                    requires_service_probe: false,
                    requires_runtime_binding_probe: false,
                    requires_16k_page_compatibility: true,
                },
                RuntimeComponentSpec {
                    component_id: "omniroute".into(),
                    version_requirement: "3.8.50".into(),
                    version_requirement_pinned: true,
                    artifact_kind: RuntimeArtifactKind::DataBundle,
                    placement: RuntimePlacement::ApkAsset,
                    delivery_module: None,
                    bundled_in_base: None,
                    relative_path: Some("omniroute/".into()),
                    required_for: vec![RuntimeCapability::Gateway],
                    requires_exec_probe: false,
                    requires_version_probe: false,
                    requires_unix_socket_probe: false,
                    requires_service_probe: true,
                    requires_runtime_binding_probe: false,
                    requires_16k_page_compatibility: false,
                },
                RuntimeComponentSpec {
                    component_id: "npm_cli".into(),
                    version_requirement: "compatible-with-node".into(),
                    version_requirement_pinned: true,
                    artifact_kind: RuntimeArtifactKind::DataBundle,
                    placement: RuntimePlacement::ApkAsset,
                    delivery_module: None,
                    bundled_in_base: None,
                    relative_path: Some("node/npm".into()),
                    required_for: vec![RuntimeCapability::WebsiteBuild],
                    requires_exec_probe: false,
                    requires_version_probe: false,
                    requires_unix_socket_probe: false,
                    requires_service_probe: false,
                    requires_runtime_binding_probe: true,
                    requires_16k_page_compatibility: false,
                },
                RuntimeComponentSpec {
                    component_id: "java".into(),
                    version_requirement: "21.0.0".into(),
                    version_requirement_pinned: true,
                    artifact_kind: RuntimeArtifactKind::NativeExecutable,
                    placement: RuntimePlacement::ApkNativeExecutable,
                    delivery_module: None,
                    bundled_in_base: None,
                    relative_path: Some("libvibecoder_java_exec.so".into()),
                    required_for: vec![RuntimeCapability::AndroidBuild],
                    requires_exec_probe: true,
                    requires_version_probe: true,
                    requires_unix_socket_probe: false,
                    requires_service_probe: false,
                    requires_runtime_binding_probe: false,
                    requires_16k_page_compatibility: true,
                },
            ],
        }
    }

    #[test]
    fn baseline_inventory_validates() {
        baseline().validate().expect("baseline inventory");
    }

    #[test]
    fn writable_app_data_cannot_be_native_executable() {
        let mut inventory = baseline();
        let jcode = inventory
            .components
            .iter_mut()
            .find(|component| component.component_id == "jcode")
            .expect("jcode");
        jcode.placement = RuntimePlacement::WritableAppData;
        assert!(inventory.validate().is_err());
    }

    #[test]
    fn unpinned_requirement_blocks_capability_even_with_other_proof() {
        let mut inventory = baseline();
        let core = inventory
            .components
            .iter_mut()
            .find(|component| component.component_id == "vibecoder_core")
            .expect("core");
        core.version_requirement_pinned = false;
        let evidence = vec![RuntimeComponentEvidence {
            component_id: "vibecoder_core".into(),
            package_presence: ProbeState::Passed,
            arm64_identity: ProbeState::Passed,
            execution: ProbeState::NotRun,
            version: ProbeState::NotRun,
            unix_socket_round_trip: ProbeState::NotRun,
            service_round_trip: ProbeState::NotRun,
            runtime_binding: ProbeState::NotRun,
            page_size_16k_compatibility: ProbeState::Passed,
            expected_version_requirement: None,
            observed_version: None,
        }];
        let report = evaluate_android_arm64_readiness(&inventory, &evidence).expect("report");
        assert!(!report.core_ready);
        assert!(report.blockers.iter().any(|blocker| {
            blocker.component_id == "vibecoder_core"
                && blocker.code == "runtime_version_requirement_unpinned"
        }));
    }

    #[test]
    fn pinned_version_probe_rejects_unstructured_requirement() {
        let mut inventory = baseline();
        let java = inventory
            .components
            .iter_mut()
            .find(|component| component.component_id == "java")
            .expect("java");
        java.version_requirement = "Android ARM64-compatible JDK".into();
        assert!(inventory.validate().is_err());
    }

    #[test]
    fn absent_device_proofs_fail_closed() {
        let report = evaluate_android_arm64_readiness(&baseline(), &[]).expect("report");
        assert!(!report.backend_ready());
        assert!(!report.website_build_ready);
        assert!(!report.android_build_ready);
        assert!(report.blockers.iter().any(|blocker| {
            blocker.component_id == "jcode" && blocker.code == "runtime_execution_unproven"
        }));
    }

    #[test]
    fn exact_required_proofs_can_enable_backend_without_claiming_android_build() {
        let inventory = baseline();
        let passed = ProbeState::Passed;
        let evidence = vec![
            RuntimeComponentEvidence {
                component_id: "vibecoder_core".into(),
                package_presence: passed,
                arm64_identity: passed,
                execution: ProbeState::NotRun,
                version: ProbeState::NotRun,
                unix_socket_round_trip: ProbeState::NotRun,
                service_round_trip: ProbeState::NotRun,
                runtime_binding: ProbeState::NotRun,
                page_size_16k_compatibility: passed,
                expected_version_requirement: None,
                observed_version: None,
            },
            RuntimeComponentEvidence {
                component_id: "jcode".into(),
                package_presence: passed,
                arm64_identity: passed,
                execution: passed,
                version: passed,
                unix_socket_round_trip: passed,
                service_round_trip: ProbeState::NotRun,
                runtime_binding: ProbeState::NotRun,
                page_size_16k_compatibility: passed,
                expected_version_requirement: None,
                observed_version: None,
            },
            RuntimeComponentEvidence {
                component_id: "node".into(),
                package_presence: passed,
                arm64_identity: passed,
                execution: passed,
                version: passed,
                unix_socket_round_trip: ProbeState::NotRun,
                service_round_trip: ProbeState::NotRun,
                runtime_binding: ProbeState::NotRun,
                page_size_16k_compatibility: passed,
                expected_version_requirement: None,
                observed_version: None,
            },
            RuntimeComponentEvidence {
                component_id: "omniroute".into(),
                package_presence: passed,
                arm64_identity: ProbeState::NotRun,
                execution: ProbeState::NotRun,
                version: ProbeState::NotRun,
                unix_socket_round_trip: ProbeState::NotRun,
                service_round_trip: passed,
                runtime_binding: ProbeState::NotRun,
                page_size_16k_compatibility: ProbeState::NotRun,
                expected_version_requirement: None,
                observed_version: None,
            },
            RuntimeComponentEvidence {
                component_id: "npm_cli".into(),
                package_presence: passed,
                arm64_identity: ProbeState::NotRun,
                execution: ProbeState::NotRun,
                version: ProbeState::NotRun,
                unix_socket_round_trip: ProbeState::NotRun,
                service_round_trip: ProbeState::NotRun,
                runtime_binding: passed,
                page_size_16k_compatibility: ProbeState::NotRun,
                expected_version_requirement: None,
                observed_version: None,
            },
        ];
        let report = evaluate_android_arm64_readiness(&inventory, &evidence).expect("report");
        assert!(report.backend_ready());
        assert!(report.website_build_ready);
        assert!(!report.android_build_ready);
    }

    #[test]
    fn omniroute_asset_presence_alone_does_not_prove_gateway_service() {
        let inventory = baseline();
        let passed = ProbeState::Passed;
        let evidence = vec![RuntimeComponentEvidence {
            component_id: "omniroute".into(),
            package_presence: passed,
            arm64_identity: ProbeState::NotRun,
            execution: ProbeState::NotRun,
            version: ProbeState::NotRun,
            unix_socket_round_trip: ProbeState::NotRun,
            service_round_trip: ProbeState::NotRun,
            runtime_binding: ProbeState::NotRun,
            page_size_16k_compatibility: ProbeState::NotRun,
            expected_version_requirement: None,
            observed_version: None,
        }];
        let report = evaluate_android_arm64_readiness(&inventory, &evidence).expect("report");
        assert!(!report.gateway_ready);
        assert!(report.blockers.iter().any(|blocker| {
            blocker.component_id == "omniroute"
                && blocker.code == "runtime_service_round_trip_unproven"
        }));
    }

    #[test]
    fn npm_asset_presence_without_node_binding_does_not_prove_website_build() {
        let inventory = baseline();
        let passed = ProbeState::Passed;
        let evidence = vec![RuntimeComponentEvidence {
            component_id: "npm_cli".into(),
            package_presence: passed,
            arm64_identity: ProbeState::NotRun,
            execution: ProbeState::NotRun,
            version: ProbeState::NotRun,
            unix_socket_round_trip: ProbeState::NotRun,
            service_round_trip: ProbeState::NotRun,
            runtime_binding: ProbeState::NotRun,
            page_size_16k_compatibility: ProbeState::NotRun,
            expected_version_requirement: None,
            observed_version: None,
        }];
        let report = evaluate_android_arm64_readiness(&inventory, &evidence).expect("report");
        assert!(!report.website_build_ready);
        assert!(report.blockers.iter().any(|blocker| {
            blocker.component_id == "npm_cli" && blocker.code == "runtime_binding_unproven"
        }));
    }

    #[test]
    fn traversal_component_path_is_rejected() {
        let mut inventory = baseline();
        inventory.components[0].relative_path = Some("../libbad.so".into());
        assert!(inventory.validate().is_err());
    }

    #[test]
    fn java_generated_evidence_deserializes_correctly() {
        // This JSON matches the format produced by MainActivity.java's buildApkAssetEvidence.
        // It specifically verifies that "passed" and "failed" (snake_case) work as expected
        // for the ProbeState enum.
        let json = r#"[{"component_id":"jcode","package_presence":"passed"},{"component_id":"node","package_presence":"failed"}]"#;
        let evidence: Vec<RuntimeComponentEvidence> = serde_json::from_str(json).expect("valid evidence json");
        assert_eq!(evidence.len(), 2);
        
        assert_eq!(evidence[0].component_id, "jcode");
        assert_eq!(evidence[0].package_presence, ProbeState::Passed);
        assert_eq!(evidence[0].arm64_identity, ProbeState::NotRun); // Default
        
        assert_eq!(evidence[1].component_id, "node");
        assert_eq!(evidence[1].package_presence, ProbeState::Failed);
    }
}
