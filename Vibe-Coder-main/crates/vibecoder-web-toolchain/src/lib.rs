//! Read-only website toolchain detection for VibeCoder Part 19.
//!
//! This crate does not execute package-manager scripts, resolve executable paths, install
//! dependencies, or use ambient PATH as runtime authority. It only inspects bounded project metadata through the safe
//! workspace runtime and returns a deterministic logical build intent for Part 20.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use vibecoder_domain::{ProjectRef, Result, VibeCoderError};
use vibecoder_workspace_contract::WorkspaceRuntime;

pub const MAX_PACKAGE_JSON_BYTES: usize = 1024 * 1024;
pub const MAX_LOCKFILE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PACKAGE_NAME_BYTES: usize = 256;
pub const MAX_RUNTIME_REQUIREMENT_BYTES: usize = 256;
pub const BUILD_SCRIPT_NAME: &str = "build";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl PackageManager {
    /// Logical runtime-registry id only. This is never an executable path or PATH lookup.
    pub const fn runtime_tool_id(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Bun => "bun",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebFramework {
    Static,
    Vite,
    React,
    Vue,
    NextJs,
    Angular,
    GenericNode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRuntimeRequirement {
    /// Package-controlled advisory constraint from `engines.node`; Part 20 must corroborate it
    /// against a provisioned trusted Node runtime before execution.
    engines_node: Option<String>,
}

impl NodeRuntimeRequirement {
    pub fn engines_node(&self) -> Option<&str> {
        self.engines_node.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyInstallIntent {
    /// A lockfile exists and Part 20 should use the package manager's lockfile-preserving install.
    Locked,
    /// No lockfile exists. Dependency installation requires an explicit later policy decision.
    Unlocked,
    /// Static project with no package manager.
    NotRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebsiteBuildIntent {
    package_manager: Option<PackageManager>,
    install: DependencyInstallIntent,
    build_script_name: Option<&'static str>,
}

impl WebsiteBuildIntent {
    pub const fn package_manager(&self) -> Option<PackageManager> {
        self.package_manager
    }
    pub fn install_intent(&self) -> &DependencyInstallIntent {
        &self.install
    }
    pub const fn build_script_name(&self) -> Option<&'static str> {
        self.build_script_name
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebsiteToolchainReport {
    framework: WebFramework,
    package_manager: Option<PackageManager>,
    package_manager_declaration: Option<String>,
    node_requirement: Option<NodeRuntimeRequirement>,
    package_name: Option<String>,
    has_build_script: bool,
    build_intent: WebsiteBuildIntent,
    manifest_sha256_hex: String,
    lockfile_relative_path: Option<String>,
    lockfile_sha256_hex: Option<String>,
}

impl WebsiteToolchainReport {
    pub const fn framework(&self) -> WebFramework {
        self.framework
    }
    pub const fn package_manager(&self) -> Option<PackageManager> {
        self.package_manager
    }
    pub fn package_manager_declaration(&self) -> Option<&str> {
        self.package_manager_declaration.as_deref()
    }
    pub fn node_requirement(&self) -> Option<&NodeRuntimeRequirement> {
        self.node_requirement.as_ref()
    }
    pub fn package_name(&self) -> Option<&str> {
        self.package_name.as_deref()
    }
    pub const fn has_build_script(&self) -> bool {
        self.has_build_script
    }
    pub fn build_intent(&self) -> &WebsiteBuildIntent {
        &self.build_intent
    }
    /// SHA-256 of the exact package.json bytes inspected for this report. Part 20 re-inspects the
    /// project before approval/start and requires the full report, including this digest, to match.
    pub fn manifest_sha256_hex(&self) -> &str {
        &self.manifest_sha256_hex
    }
    pub fn lockfile_relative_path(&self) -> Option<&str> {
        self.lockfile_relative_path.as_deref()
    }
    pub fn lockfile_sha256_hex(&self) -> Option<&str> {
        self.lockfile_sha256_hex.as_deref()
    }
}

pub async fn inspect_website_project<W: WorkspaceRuntime>(
    workspace: &W,
    project: &ProjectRef,
) -> Result<WebsiteToolchainReport> {
    workspace.verify_project(project).await?;
    // Root metadata detection must not recursively enumerate node_modules; after a successful
    // dependency install that tree can contain tens of thousands of files. Probe only the small
    // fixed set of files that influence package-manager/framework selection.
    let mut files = HashSet::new();
    for relative in [
        "package.json",
        "index.html",
        "package-lock.json",
        "npm-shrinkwrap.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "bun.lock",
        "bun.lockb",
        "next.config.js",
        "next.config.mjs",
        "next.config.ts",
        "angular.json",
        "vite.config.js",
        "vite.config.ts",
        "vite.config.mjs",
    ] {
        if workspace
            .regular_file_exists(project, std::path::Path::new(relative))
            .await?
        {
            files.insert(relative.to_owned());
        }
    }
    let has_package_json = files.contains("package.json");
    let has_index_html = files.contains("index.html");

    if !has_package_json {
        if has_index_html {
            return Ok(WebsiteToolchainReport {
                framework: WebFramework::Static,
                package_manager: None,
                package_manager_declaration: None,
                node_requirement: None,
                package_name: None,
                has_build_script: false,
                build_intent: WebsiteBuildIntent {
                    package_manager: None,
                    install: DependencyInstallIntent::NotRequired,
                    build_script_name: None,
                },
                manifest_sha256_hex: String::new(),
                lockfile_relative_path: None,
                lockfile_sha256_hex: None,
            });
        }
        return Err(toolchain_error("web_toolchain_package_json_missing"));
    }

    let package_bytes = workspace
        .read_file(
            project,
            std::path::Path::new("package.json"),
            MAX_PACKAGE_JSON_BYTES,
        )
        .await?;
    let manifest_sha256_hex = format!("{:x}", Sha256::digest(&package_bytes));
    let package_text = std::str::from_utf8(&package_bytes)
        .map_err(|_| toolchain_error("web_toolchain_package_json_non_utf8"))?;
    let root: Value = serde_json::from_str(package_text)
        .map_err(|_| toolchain_error("web_toolchain_package_json_invalid"))?;
    let object = root
        .as_object()
        .ok_or_else(|| toolchain_error("web_toolchain_package_json_root_invalid"))?;

    let package_name = optional_bounded_string(
        object,
        "name",
        MAX_PACKAGE_NAME_BYTES,
        "web_toolchain_package_name_invalid",
    )?;
    let node_requirement = parse_node_requirement(object)?;
    let package_manager_field = parse_package_manager_field(object)?;
    let field_manager = package_manager_field.as_ref().map(|(manager, _)| *manager);
    let package_manager_declaration = package_manager_field.map(|(_, declaration)| declaration);
    let detected_lockfile = detect_lockfile_manager(&files)?;
    let lock_manager = detected_lockfile.as_ref().map(|(manager, _)| *manager);
    let package_manager = reconcile_package_manager(field_manager, lock_manager)?;
    let (lockfile_relative_path, lockfile_sha256_hex) =
        if let Some((_, relative)) = detected_lockfile {
            let bytes = workspace
                .read_file(project, std::path::Path::new(relative), MAX_LOCKFILE_BYTES)
                .await?;
            (
                Some(relative.to_owned()),
                Some(format!("{:x}", Sha256::digest(&bytes))),
            )
        } else {
            (None, None)
        };

    let dependencies = dependency_names(object)?;
    let framework = detect_framework(&dependencies, &files);
    let has_build_script = has_string_script(object, BUILD_SCRIPT_NAME)?;

    let install = if lock_manager.is_some() {
        DependencyInstallIntent::Locked
    } else {
        DependencyInstallIntent::Unlocked
    };

    Ok(WebsiteToolchainReport {
        framework,
        package_manager: Some(package_manager),
        package_manager_declaration,
        node_requirement: Some(node_requirement),
        package_name,
        has_build_script,
        build_intent: WebsiteBuildIntent {
            package_manager: Some(package_manager),
            install,
            build_script_name: has_build_script.then_some(BUILD_SCRIPT_NAME),
        },
        manifest_sha256_hex,
        lockfile_relative_path,
        lockfile_sha256_hex,
    })
}

fn detect_lockfile_manager(
    files: &HashSet<String>,
) -> Result<Option<(PackageManager, &'static str)>> {
    let npm_lock = if files.contains("npm-shrinkwrap.json") {
        Some("npm-shrinkwrap.json")
    } else if files.contains("package-lock.json") {
        Some("package-lock.json")
    } else {
        None
    };
    if files.contains("bun.lock") && files.contains("bun.lockb") {
        return Err(toolchain_error("web_toolchain_multiple_bun_lockfiles"));
    }
    let bun_lock = if files.contains("bun.lock") {
        Some("bun.lock")
    } else if files.contains("bun.lockb") {
        Some("bun.lockb")
    } else {
        None
    };

    let mut found = Vec::new();
    if let Some(path) = npm_lock {
        found.push((PackageManager::Npm, path));
    }
    if files.contains("pnpm-lock.yaml") {
        found.push((PackageManager::Pnpm, "pnpm-lock.yaml"));
    }
    if files.contains("yarn.lock") {
        found.push((PackageManager::Yarn, "yarn.lock"));
    }
    if let Some(path) = bun_lock {
        found.push((PackageManager::Bun, path));
    }

    match found.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(*one)),
        _ => Err(toolchain_error("web_toolchain_multiple_lockfiles")),
    }
}

fn parse_package_manager_field(
    object: &Map<String, Value>,
) -> Result<Option<(PackageManager, String)>> {
    let Some(value) = object.get("packageManager") else {
        return Ok(None);
    };
    let text = value
        .as_str()
        .ok_or_else(|| toolchain_error("web_toolchain_package_manager_field_invalid"))?;
    validate_metadata_text(
        text,
        MAX_RUNTIME_REQUIREMENT_BYTES,
        "web_toolchain_package_manager_field_invalid",
    )?;
    let name = text.split('@').next().unwrap_or(text);
    let manager = match name {
        "npm" => PackageManager::Npm,
        "pnpm" => PackageManager::Pnpm,
        "yarn" => PackageManager::Yarn,
        "bun" => PackageManager::Bun,
        _ => return Err(toolchain_error("web_toolchain_package_manager_unsupported")),
    };
    Ok(Some((manager, text.to_owned())))
}

fn reconcile_package_manager(
    field: Option<PackageManager>,
    lockfile: Option<PackageManager>,
) -> Result<PackageManager> {
    match (field, lockfile) {
        (Some(a), Some(b)) if a != b => {
            Err(toolchain_error("web_toolchain_package_manager_conflict"))
        }
        (Some(manager), _) | (_, Some(manager)) => Ok(manager),
        (None, None) => Err(toolchain_error("web_toolchain_package_manager_unknown")),
    }
}

fn parse_node_requirement(object: &Map<String, Value>) -> Result<NodeRuntimeRequirement> {
    let engines_node = match object.get("engines") {
        None => None,
        Some(Value::Object(engines)) => optional_bounded_string(
            engines,
            "node",
            MAX_RUNTIME_REQUIREMENT_BYTES,
            "web_toolchain_node_engine_invalid",
        )?,
        Some(_) => return Err(toolchain_error("web_toolchain_engines_invalid")),
    };
    Ok(NodeRuntimeRequirement { engines_node })
}

fn dependency_names(object: &Map<String, Value>) -> Result<HashSet<String>> {
    let mut out = HashSet::new();
    for key in ["dependencies", "devDependencies", "peerDependencies"] {
        let Some(value) = object.get(key) else {
            continue;
        };
        let entries = value
            .as_object()
            .ok_or_else(|| toolchain_error("web_toolchain_dependencies_invalid"))?;
        for name in entries.keys() {
            if name.len() > MAX_PACKAGE_NAME_BYTES
                || name.is_empty()
                || name
                    .chars()
                    .any(|ch| ch.is_control() || is_bidi_control(ch))
            {
                return Err(toolchain_error("web_toolchain_dependency_name_invalid"));
            }
            out.insert(name.clone());
        }
    }
    Ok(out)
}

fn detect_framework(dependencies: &HashSet<String>, files: &HashSet<String>) -> WebFramework {
    if dependencies.contains("next")
        || files.contains("next.config.js")
        || files.contains("next.config.mjs")
        || files.contains("next.config.ts")
    {
        WebFramework::NextJs
    } else if dependencies.contains("@angular/core") || files.contains("angular.json") {
        WebFramework::Angular
    } else if dependencies.contains("vite")
        || files.contains("vite.config.js")
        || files.contains("vite.config.ts")
        || files.contains("vite.config.mjs")
    {
        WebFramework::Vite
    } else if dependencies.contains("vue") {
        WebFramework::Vue
    } else if dependencies.contains("react") {
        WebFramework::React
    } else {
        WebFramework::GenericNode
    }
}

fn has_string_script(object: &Map<String, Value>, name: &str) -> Result<bool> {
    let Some(value) = object.get("scripts") else {
        return Ok(false);
    };
    let scripts = value
        .as_object()
        .ok_or_else(|| toolchain_error("web_toolchain_scripts_invalid"))?;
    let Some(script) = scripts.get(name) else {
        return Ok(false);
    };
    let text = script
        .as_str()
        .ok_or_else(|| toolchain_error("web_toolchain_build_script_invalid"))?;
    if text.is_empty() || text.len() > 64 * 1024 {
        return Err(toolchain_error("web_toolchain_build_script_invalid"));
    }
    // The script body is intentionally neither returned nor authorized by this crate.
    Ok(true)
}

fn optional_bounded_string(
    object: &Map<String, Value>,
    key: &str,
    max_bytes: usize,
    error: &'static str,
) -> Result<Option<String>> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    let text = value.as_str().ok_or_else(|| toolchain_error(error))?;
    validate_metadata_text(text, max_bytes, error)?;
    Ok(Some(text.to_owned()))
}

fn validate_metadata_text(text: &str, max_bytes: usize, error: &'static str) -> Result<()> {
    if text.is_empty()
        || text.len() > max_bytes
        || text
            .chars()
            .any(|ch| ch.is_control() || is_bidi_control(ch))
    {
        return Err(toolchain_error(error));
    }
    Ok(())
}

fn is_bidi_control(ch: char) -> bool {
    matches!(
        ch,
        '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'
            | '\u{202b}'
            | '\u{202c}'
            | '\u{202d}'
            | '\u{202e}'
            | '\u{2066}'
            | '\u{2067}'
            | '\u{2068}'
            | '\u{2069}'
    )
}

fn toolchain_error(code: impl Into<String>) -> VibeCoderError {
    VibeCoderError::Build(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflicting_package_manager_sources_fail_closed() {
        assert_eq!(
            reconcile_package_manager(Some(PackageManager::Npm), Some(PackageManager::Pnpm))
                .unwrap_err()
                .to_string(),
            "build job error: web_toolchain_package_manager_conflict"
        );
    }

    #[test]
    fn multiple_lock_managers_are_rejected() {
        let files = HashSet::from(["package-lock.json".to_owned(), "yarn.lock".to_owned()]);
        assert!(detect_lockfile_manager(&files).is_err());
    }

    #[test]
    fn runtime_tool_ids_are_fixed_not_package_controlled() {
        assert_eq!(PackageManager::Npm.runtime_tool_id(), "npm");
        assert_eq!(PackageManager::Pnpm.runtime_tool_id(), "pnpm");
        assert_eq!(PackageManager::Yarn.runtime_tool_id(), "yarn");
        assert_eq!(PackageManager::Bun.runtime_tool_id(), "bun");
    }

    #[test]
    fn framework_precedence_is_deterministic() {
        let deps = HashSet::from(["react".to_owned(), "next".to_owned(), "vite".to_owned()]);
        assert_eq!(
            detect_framework(&deps, &HashSet::new()),
            WebFramework::NextJs
        );
    }
}
