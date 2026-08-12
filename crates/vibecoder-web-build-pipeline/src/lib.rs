//! Website build pipeline state machine for VibeCoder Part 20.
//!
//! This crate converts a Part-19 read-only toolchain report into exact structured package-manager
//! commands and advances only from real Part-18 build results. It does not approve commands, spawn
//! processes, resolve executable paths, inspect ambient PATH, or claim that a successful process
//! produced a verified deployable artifact.

use std::fmt;
use std::path::PathBuf;
use uuid::Uuid;
use vibecoder_build_contract::{BuildResult, BuildState, BuildTargetKind, RunningBuildJob};
use vibecoder_command_policy::{CommandProgram, CommandSpec};
use vibecoder_domain::{ProjectId, Result, VibeCoderError};
use vibecoder_web_toolchain::{DependencyInstallIntent, PackageManager, WebsiteToolchainReport};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WebsitePipelineId(Uuid);

impl WebsitePipelineId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for WebsitePipelineId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebsiteBuildStage {
    DependencyInstall,
    BuildScript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebsiteBuildPipelineState {
    NoBuildRequired,
    AwaitingApproval(WebsiteBuildStage),
    Running(WebsiteBuildStage),
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

impl WebsiteBuildPipelineState {
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::NoBuildRequired
                | Self::Succeeded
                | Self::Failed
                | Self::Cancelled
                | Self::TimedOut
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebsiteBuildPolicy {
    /// Install dependencies before the build script. Disable only when the caller deliberately
    /// wants to reuse an already-prepared dependency tree.
    pub install_dependencies: bool,
    /// Package installation lifecycle scripts are arbitrary code. They are disabled by default.
    pub allow_dependency_install_scripts: bool,
}

impl Default for WebsiteBuildPolicy {
    fn default() -> Self {
        Self {
            install_dependencies: true,
            allow_dependency_install_scripts: false,
        }
    }
}

#[derive(PartialEq, Eq)]
pub struct WebsiteBuildPipeline {
    pipeline_id: WebsitePipelineId,
    project_id: ProjectId,
    report: WebsiteToolchainReport,
    policy: WebsiteBuildPolicy,
    state: WebsiteBuildPipelineState,
}

impl fmt::Debug for WebsiteBuildPipeline {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebsiteBuildPipeline")
            .field("pipeline_id", &self.pipeline_id)
            .field("project_id", &self.project_id)
            .field("framework", &self.report.framework())
            .field("package_manager", &self.report.package_manager())
            .field("manifest_sha256_hex", &self.report.manifest_sha256_hex())
            .field("policy", &self.policy)
            .field("state", &self.state)
            .finish()
    }
}

impl WebsiteBuildPipeline {
    pub fn new(
        project_id: ProjectId,
        report: WebsiteToolchainReport,
        policy: WebsiteBuildPolicy,
    ) -> Result<Self> {
        let state = if report.package_manager().is_none() {
            if report.has_build_script() {
                return Err(web_build_error("web_build_static_script_inconsistent"));
            }
            WebsiteBuildPipelineState::NoBuildRequired
        } else {
            if !report.has_build_script()
                || report.build_intent().build_script_name() != Some("build")
            {
                return Err(web_build_error("web_build_script_missing"));
            }
            if policy.install_dependencies {
                match report.build_intent().install_intent() {
                    DependencyInstallIntent::Locked => WebsiteBuildPipelineState::AwaitingApproval(
                        WebsiteBuildStage::DependencyInstall,
                    ),
                    DependencyInstallIntent::Unlocked => {
                        return Err(web_build_error("web_build_unlocked_install_disallowed"));
                    }
                    DependencyInstallIntent::NotRequired => {
                        return Err(web_build_error("web_build_install_intent_inconsistent"));
                    }
                }
            } else {
                WebsiteBuildPipelineState::AwaitingApproval(WebsiteBuildStage::BuildScript)
            }
        };
        Ok(Self {
            pipeline_id: WebsitePipelineId::new(),
            project_id,
            report,
            policy,
            state,
        })
    }

    pub const fn pipeline_id(&self) -> WebsitePipelineId {
        self.pipeline_id
    }
    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }
    pub const fn state(&self) -> WebsiteBuildPipelineState {
        self.state
    }
    pub fn report(&self) -> &WebsiteToolchainReport {
        &self.report
    }
    pub const fn policy(&self) -> WebsiteBuildPolicy {
        self.policy
    }

    /// Full report equality binds command generation to the same manifest hash, package manager,
    /// lockfile classification, framework detection, and build-script presence observed at plan time.
    pub fn verify_toolchain_unchanged(&self, current: &WebsiteToolchainReport) -> Result<()> {
        if &self.report != current {
            return Err(web_build_error("web_build_toolchain_changed"));
        }
        Ok(())
    }

    pub fn current_stage(&self) -> Option<WebsiteBuildStage> {
        match self.state {
            WebsiteBuildPipelineState::AwaitingApproval(stage)
            | WebsiteBuildPipelineState::Running(stage) => Some(stage),
            _ => None,
        }
    }

    /// Return the exact structured command for the currently awaiting stage. The command remains
    /// subject to Part-14 allow-once approval and Part-15 trusted runtime-tool resolution.
    pub fn current_command(&self) -> Result<CommandSpec> {
        let WebsiteBuildPipelineState::AwaitingApproval(stage) = self.state else {
            return Err(web_build_error("web_build_stage_not_awaiting_approval"));
        };
        self.command_for_stage(stage)
    }

    pub fn command_matches_current_stage(&self, command: &CommandSpec) -> Result<()> {
        if self.current_command()? != *command {
            return Err(web_build_error("web_build_authorized_command_mismatch"));
        }
        Ok(())
    }

    pub fn into_running(self, running: RunningBuildJob) -> Result<RunningWebsiteBuildStage> {
        let WebsiteBuildPipelineState::AwaitingApproval(stage) = self.state else {
            return Err(web_build_error("web_build_stage_not_awaiting_approval"));
        };
        if running.project_id() != self.project_id || running.target() != BuildTargetKind::Website {
            return Err(web_build_error("web_build_running_job_scope_mismatch"));
        }
        Ok(RunningWebsiteBuildStage {
            pipeline: Self {
                state: WebsiteBuildPipelineState::Running(stage),
                ..self
            },
            stage,
            running,
        })
    }

    fn command_for_stage(&self, stage: WebsiteBuildStage) -> Result<CommandSpec> {
        let manager = self
            .report
            .package_manager()
            .ok_or_else(|| web_build_error("web_build_package_manager_missing"))?;
        let args = match stage {
            WebsiteBuildStage::DependencyInstall => self.install_args(manager)?,
            WebsiteBuildStage::BuildScript => vec!["run".into(), "build".into()],
        };
        Ok(CommandSpec {
            program: CommandProgram::RuntimeTool {
                tool_id: manager.runtime_tool_id().to_owned(),
            },
            args,
            working_dir: PathBuf::from("."),
        })
    }

    fn install_args(&self, manager: PackageManager) -> Result<Vec<String>> {
        let locked = match self.report.build_intent().install_intent() {
            DependencyInstallIntent::Locked => true,
            DependencyInstallIntent::Unlocked => {
                return Err(web_build_error("web_build_unlocked_install_disallowed"));
            }
            DependencyInstallIntent::NotRequired => {
                return Err(web_build_error("web_build_install_intent_inconsistent"));
            }
        };

        let mut args: Vec<String> = match (manager, locked) {
            (PackageManager::Npm, true) => vec!["ci".into()],
            (PackageManager::Npm, false) => {
                return Err(web_build_error("web_build_unlocked_install_disallowed"));
            }
            (PackageManager::Pnpm, true) => vec!["install".into(), "--frozen-lockfile".into()],
            (PackageManager::Pnpm, false) => {
                return Err(web_build_error("web_build_unlocked_install_disallowed"));
            }
            (PackageManager::Yarn, true) => self.yarn_locked_install_args()?,
            (PackageManager::Yarn, false) => {
                return Err(web_build_error("web_build_unlocked_install_disallowed"));
            }
            (PackageManager::Bun, true) => vec!["install".into(), "--frozen-lockfile".into()],
            (PackageManager::Bun, false) => {
                return Err(web_build_error("web_build_unlocked_install_disallowed"));
            }
        };
        if !self.policy.allow_dependency_install_scripts && manager != PackageManager::Yarn {
            args.push("--ignore-scripts".into());
        }
        Ok(args)
    }

    fn yarn_locked_install_args(&self) -> Result<Vec<String>> {
        if self.policy.allow_dependency_install_scripts {
            // --frozen-lockfile works in Classic and remains a current backward-compatibility alias
            // for --immutable in modern Yarn.
            return Ok(vec!["install".into(), "--frozen-lockfile".into()]);
        }
        let declaration = self
            .report
            .package_manager_declaration()
            .ok_or_else(|| web_build_error("web_build_yarn_version_required_for_safe_install"))?;
        let version = declaration
            .strip_prefix("yarn@")
            .ok_or_else(|| web_build_error("web_build_yarn_version_invalid"))?;
        let major_digits: String = version
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        let major: u32 = major_digits
            .parse()
            .map_err(|_| web_build_error("web_build_yarn_version_invalid"))?;
        match major {
            0 => Err(web_build_error("web_build_yarn_version_invalid")),
            1 => Ok(vec![
                "install".into(),
                "--frozen-lockfile".into(),
                "--ignore-scripts".into(),
            ]),
            _ => Ok(vec![
                "install".into(),
                "--immutable".into(),
                "--mode=skip-build".into(),
            ]),
        }
    }

    fn finish_stage(mut self, stage: WebsiteBuildStage, result: &BuildResult) -> Result<Self> {
        if result.project_id() != self.project_id || result.target() != BuildTargetKind::Website {
            return Err(web_build_error("web_build_result_scope_mismatch"));
        }
        if self.state != WebsiteBuildPipelineState::Running(stage) {
            return Err(web_build_error("web_build_result_stage_mismatch"));
        }
        self.state = match result.state() {
            BuildState::Succeeded => match stage {
                WebsiteBuildStage::DependencyInstall => {
                    WebsiteBuildPipelineState::AwaitingApproval(WebsiteBuildStage::BuildScript)
                }
                WebsiteBuildStage::BuildScript => WebsiteBuildPipelineState::Succeeded,
            },
            BuildState::Failed => WebsiteBuildPipelineState::Failed,
            BuildState::Cancelled => WebsiteBuildPipelineState::Cancelled,
            BuildState::TimedOut => WebsiteBuildPipelineState::TimedOut,
            BuildState::Queued | BuildState::Running => {
                return Err(web_build_error("web_build_nonterminal_stage_result"));
            }
        };
        Ok(self)
    }
}

pub struct RunningWebsiteBuildStage {
    pipeline: WebsiteBuildPipeline,
    stage: WebsiteBuildStage,
    running: RunningBuildJob,
}

impl fmt::Debug for RunningWebsiteBuildStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RunningWebsiteBuildStage")
            .field("pipeline_id", &self.pipeline.pipeline_id)
            .field("project_id", &self.pipeline.project_id)
            .field("stage", &self.stage)
            .field("build_id", &self.running.build_id())
            .field("process_id", &self.running.process_id())
            .finish_non_exhaustive()
    }
}

impl RunningWebsiteBuildStage {
    pub const fn pipeline_id(&self) -> WebsitePipelineId {
        self.pipeline.pipeline_id
    }
    pub const fn project_id(&self) -> ProjectId {
        self.pipeline.project_id
    }
    pub const fn stage(&self) -> WebsiteBuildStage {
        self.stage
    }
    pub const fn build_id(&self) -> vibecoder_build_contract::BuildId {
        self.running.build_id()
    }
    pub const fn process_id(&self) -> vibecoder_process_contract::ProcessId {
        self.running.process_id()
    }

    pub fn drain_events(
        &self,
        max_events: usize,
    ) -> Result<Vec<vibecoder_build_contract::BuildEvent>> {
        self.running.drain_events(max_events)
    }

    pub async fn wait(self) -> Result<WebsiteBuildStageCompletion> {
        let result = self.running.wait().await?;
        let pipeline = self.pipeline.finish_stage(self.stage, &result)?;
        Ok(WebsiteBuildStageCompletion {
            stage: self.stage,
            pipeline,
            result,
        })
    }
}

pub struct WebsiteBuildStageCompletion {
    stage: WebsiteBuildStage,
    pipeline: WebsiteBuildPipeline,
    result: BuildResult,
}

impl fmt::Debug for WebsiteBuildStageCompletion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebsiteBuildStageCompletion")
            .field("stage", &self.stage)
            .field("pipeline", &self.pipeline)
            .field("result", &self.result)
            .finish()
    }
}

impl WebsiteBuildStageCompletion {
    pub const fn stage(&self) -> WebsiteBuildStage {
        self.stage
    }
    pub fn pipeline(&self) -> &WebsiteBuildPipeline {
        &self.pipeline
    }
    pub fn result(&self) -> &BuildResult {
        &self.result
    }
    pub fn into_parts(self) -> (WebsiteBuildPipeline, BuildResult) {
        (self.pipeline, self.result)
    }
}

fn web_build_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Build(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibecoder_web_toolchain::{WebFramework, WebsiteBuildIntent};

    // Full project/report fixtures live in the Part-20 source validation because report fields are
    // intentionally private. These unit tests keep authority-free invariants close to the crate.
    #[test]
    fn pipeline_states_mark_only_terminal_states_terminal() {
        assert!(WebsiteBuildPipelineState::Succeeded.is_terminal());
        assert!(WebsiteBuildPipelineState::NoBuildRequired.is_terminal());
        assert!(
            !WebsiteBuildPipelineState::AwaitingApproval(WebsiteBuildStage::BuildScript)
                .is_terminal()
        );
        let _ = (
            WebFramework::Static,
            std::mem::size_of::<WebsiteBuildIntent>(),
        );
    }
}
