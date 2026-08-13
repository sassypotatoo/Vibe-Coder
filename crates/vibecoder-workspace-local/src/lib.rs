//! Phone-local workspace-root ownership and canonical path containment.
//!
//! The Android platform layer will pass its already-created app-private files/data directory to
//! `LocalWorkspaceRuntime::initialize`. VibeCoder creates only fixed-name descendants beneath that
//! canonical directory and project directories named from `ProjectId` values. User/model supplied
//! absolute paths never participate in project-root selection.

use async_trait::async_trait;
use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use vibecoder_domain::{ProjectId, ProjectRef, Result, VibeCoderError};
use vibecoder_workspace_contract::{
    ProjectFileList, ProjectTextSearchResult, TextEditResult, TextPatchHunk, TextPatchResult,
    WorkspaceCapabilities, WorkspaceRuntime, WorkspaceSpec,
};

#[cfg(unix)]
mod unix_io;

const PRODUCT_ROOT_NAME: &str = "vibecoder";
const PROJECTS_ROOT_NAME: &str = "projects";
const MAX_RELATIVE_PATH_BYTES: usize = 4096;
const MAX_COMPONENT_BYTES: usize = 255;
const INTERNAL_TEMP_PREFIX: &str = ".vibecoder-tmp-";
pub const MAX_FILE_READ_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_FILE_WRITE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PROJECT_LIST_ENTRIES: usize = 4096;
pub const MAX_PROJECT_SEARCH_MATCHES: usize = 512;
pub const MAX_PROJECT_SEARCH_FILE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PROJECT_SEARCH_TOTAL_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_PROJECT_SEARCH_FILES: usize = 4096;
pub const MAX_PROJECT_SEARCH_DEPTH: usize = 64;
pub const MAX_PROJECT_WALK_ENTRIES: usize = 16_384;
pub const MAX_TEXT_EDIT_EXPECTED_BYTES: usize = 1024 * 1024;
pub const MAX_TEXT_PATCH_HUNKS: usize = 64;
pub const MAX_TEXT_PATCH_INPUT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalWorkspaceConfig {
    /// Existing application-private directory supplied by the Android/platform layer.
    pub app_private_dir: PathBuf,
}

#[derive(Debug)]
pub struct LocalWorkspaceRuntime {
    app_private_root: PathBuf,
    projects_root: PathBuf,
}

impl LocalWorkspaceRuntime {
    pub fn initialize(config: LocalWorkspaceConfig) -> Result<Self> {
        if !config.app_private_dir.is_absolute() {
            return Err(workspace_error("app_private_root_not_absolute"));
        }

        reject_symlink_if_present(&config.app_private_dir, "app_private_root_is_symlink")?;
        let app_private_root =
            canonical_existing_dir(&config.app_private_dir, "app_private_root_unavailable")?;

        let product_root = create_or_verify_fixed_child(
            &app_private_root,
            PRODUCT_ROOT_NAME,
            "product_root_invalid",
        )?;
        let projects_root = create_or_verify_fixed_child(
            &product_root,
            PROJECTS_ROOT_NAME,
            "projects_root_invalid",
        )?;

        if !projects_root.starts_with(&app_private_root) {
            return Err(workspace_error("projects_root_escaped_app_private_root"));
        }

        Ok(Self {
            app_private_root,
            projects_root,
        })
    }

    pub fn app_private_root(&self) -> &Path {
        &self.app_private_root
    }

    pub fn projects_root(&self) -> &Path {
        &self.projects_root
    }

    fn expected_project_root(&self, id: ProjectId) -> PathBuf {
        self.projects_root.join(id.0.hyphenated().to_string())
    }

    fn verify_project_sync(&self, project: &ProjectRef) -> Result<PathBuf> {
        if !project.root.is_absolute() {
            return Err(workspace_error("project_root_not_absolute"));
        }

        let expected = self.expected_project_root(project.id);
        if project.root != expected {
            return Err(workspace_error("project_root_does_not_match_project_id"));
        }

        reject_symlink_if_present(&expected, "project_root_is_symlink")?;
        let canonical = canonical_existing_dir(&expected, "project_root_unavailable")?;
        if canonical != expected || !canonical.starts_with(&self.projects_root) {
            return Err(workspace_error("project_root_failed_canonical_containment"));
        }

        Ok(canonical)
    }

    fn create_project_sync(&self, spec: WorkspaceSpec) -> Result<ProjectRef> {
        let root = self.expected_project_root(spec.id());
        match fs::create_dir(&root) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                return Err(workspace_error("project_already_exists"));
            }
            Err(_) => return Err(workspace_error("project_create_failed")),
        }

        let project = ProjectRef {
            id: spec.id(),
            root,
        };
        if let Err(error) = self.verify_project_sync(&project) {
            let _ = fs::remove_dir(&project.root);
            return Err(error);
        }
        Ok(project)
    }

    fn open_project_sync(&self, id: ProjectId) -> Result<ProjectRef> {
        let project = ProjectRef {
            id,
            root: self.expected_project_root(id),
        };
        self.verify_project_sync(&project)?;
        Ok(project)
    }

    fn remove_project_sync(&self, project: &ProjectRef) -> Result<()> {
        let verified = self.verify_project_sync(project)?;
        if verified == self.projects_root || verified == self.app_private_root {
            return Err(workspace_error("refusing_to_remove_workspace_root"));
        }

        fs::remove_dir_all(&verified).map_err(|_| workspace_error("project_remove_failed"))?;
        if verified.exists() {
            return Err(workspace_error("project_remove_incomplete"));
        }
        Ok(())
    }

    fn resolve_project_path_sync(&self, project: &ProjectRef, relative: &Path) -> Result<PathBuf> {
        let project_root = self.verify_project_sync(project)?;
        let components = validate_relative_path(relative)?;

        let mut candidate = project_root.clone();
        let mut missing_ancestor_seen = false;

        for (index, component) in components.iter().enumerate() {
            candidate.push(component);
            if missing_ancestor_seen {
                continue;
            }

            match fs::symlink_metadata(&candidate) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() {
                        return Err(workspace_error("project_path_symlink_forbidden"));
                    }
                    if index + 1 < components.len() && !metadata.is_dir() {
                        return Err(workspace_error("project_path_parent_not_directory"));
                    }
                    let canonical = fs::canonicalize(&candidate)
                        .map_err(|_| workspace_error("project_path_canonicalize_failed"))?;
                    if !canonical.starts_with(&project_root) {
                        return Err(workspace_error("project_path_escaped_project_root"));
                    }
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    missing_ancestor_seen = true;
                }
                Err(_) => return Err(workspace_error("project_path_metadata_failed")),
            }
        }

        if !candidate.starts_with(&project_root) {
            return Err(workspace_error("project_path_escaped_project_root"));
        }
        Ok(candidate)
    }

    fn create_dir_all_sync(&self, project: &ProjectRef, relative: &Path) -> Result<()> {
        #[cfg(unix)]
        {
            unix_io::create_dir_all(self, project, relative)
        }
        #[cfg(not(unix))]
        {
            let _ = (project, relative);
            Err(workspace_error("secure_file_io_unsupported_platform"))
        }
    }

    fn read_file_sync(
        &self,
        project: &ProjectRef,
        relative: &Path,
        max_bytes: usize,
    ) -> Result<Vec<u8>> {
        #[cfg(unix)]
        {
            unix_io::read_file(self, project, relative, max_bytes)
        }
        #[cfg(not(unix))]
        {
            let _ = (project, relative, max_bytes);
            Err(workspace_error("secure_file_io_unsupported_platform"))
        }
    }

    fn regular_file_exists_sync(&self, project: &ProjectRef, relative: &Path) -> Result<bool> {
        #[cfg(unix)]
        {
            unix_io::regular_file_exists(self, project, relative)
        }
        #[cfg(not(unix))]
        {
            let _ = (project, relative);
            Err(workspace_error("secure_file_io_unsupported_platform"))
        }
    }

    fn atomic_write_file_sync(
        &self,
        project: &ProjectRef,
        relative: &Path,
        contents: &[u8],
    ) -> Result<()> {
        #[cfg(unix)]
        {
            unix_io::atomic_write_file(self, project, relative, contents)
        }
        #[cfg(not(unix))]
        {
            let _ = (project, relative, contents);
            Err(workspace_error("secure_file_io_unsupported_platform"))
        }
    }

    fn edit_text_file_sync(
        &self,
        project: &ProjectRef,
        relative: &Path,
        expected: &str,
        replacement: &str,
    ) -> Result<TextEditResult> {
        #[cfg(unix)]
        {
            unix_io::edit_text_file(self, project, relative, expected, replacement)
        }
        #[cfg(not(unix))]
        {
            let _ = (project, relative, expected, replacement);
            Err(workspace_error("secure_file_io_unsupported_platform"))
        }
    }

    fn apply_text_patch_sync(
        &self,
        project: &ProjectRef,
        relative: &Path,
        hunks: &[TextPatchHunk],
    ) -> Result<TextPatchResult> {
        #[cfg(unix)]
        {
            unix_io::apply_text_patch(self, project, relative, hunks)
        }
        #[cfg(not(unix))]
        {
            let _ = (project, relative, hunks);
            Err(workspace_error("secure_file_io_unsupported_platform"))
        }
    }

    fn list_project_files_sync(
        &self,
        project: &ProjectRef,
        max_entries: usize,
    ) -> Result<ProjectFileList> {
        #[cfg(any(target_os = "android", target_os = "linux"))]
        {
            unix_io::list_project_files(self, project, max_entries)
        }
        #[cfg(not(any(target_os = "android", target_os = "linux")))]
        {
            let _ = (project, max_entries);
            Err(workspace_error(
                "secure_project_search_unsupported_platform",
            ))
        }
    }

    fn search_project_text_sync(
        &self,
        project: &ProjectRef,
        needle: &str,
        max_matches: usize,
    ) -> Result<ProjectTextSearchResult> {
        #[cfg(any(target_os = "android", target_os = "linux"))]
        {
            unix_io::search_project_text(self, project, needle, max_matches)
        }
        #[cfg(not(any(target_os = "android", target_os = "linux")))]
        {
            let _ = (project, needle, max_matches);
            Err(workspace_error(
                "secure_project_search_unsupported_platform",
            ))
        }
    }
}

#[async_trait]
impl WorkspaceRuntime for LocalWorkspaceRuntime {
    fn capabilities(&self) -> WorkspaceCapabilities {
        WorkspaceCapabilities {
            read_write_files: cfg!(unix),
            managed_project_roots: true,
            canonical_path_containment: true,
            text_edit: cfg!(unix),
            project_search: cfg!(any(target_os = "android", target_os = "linux")),
            commands: false,
            process_isolation: false,
            resource_limits: false,
            snapshots: false,
            max_file_read_bytes: if cfg!(unix) {
                MAX_FILE_READ_BYTES as u64
            } else {
                0
            },
            max_file_write_bytes: if cfg!(unix) {
                MAX_FILE_WRITE_BYTES as u64
            } else {
                0
            },
        }
    }

    async fn create_project(&self, spec: WorkspaceSpec) -> Result<ProjectRef> {
        self.create_project_sync(spec)
    }

    async fn open_project(&self, id: ProjectId) -> Result<ProjectRef> {
        self.open_project_sync(id)
    }

    async fn remove_project(&self, project: &ProjectRef) -> Result<()> {
        self.remove_project_sync(project)
    }

    async fn verify_project(&self, project: &ProjectRef) -> Result<()> {
        self.verify_project_sync(project).map(|_| ())
    }

    async fn resolve_project_path(&self, project: &ProjectRef, relative: &Path) -> Result<PathBuf> {
        self.resolve_project_path_sync(project, relative)
    }

    async fn create_dir_all(&self, project: &ProjectRef, relative: &Path) -> Result<()> {
        self.create_dir_all_sync(project, relative)
    }

    async fn read_file(
        &self,
        project: &ProjectRef,
        relative: &Path,
        max_bytes: usize,
    ) -> Result<Vec<u8>> {
        self.read_file_sync(project, relative, max_bytes)
    }

    async fn regular_file_exists(&self, project: &ProjectRef, relative: &Path) -> Result<bool> {
        self.regular_file_exists_sync(project, relative)
    }

    async fn atomic_write_file(
        &self,
        project: &ProjectRef,
        relative: &Path,
        contents: &[u8],
    ) -> Result<()> {
        self.atomic_write_file_sync(project, relative, contents)
    }

    async fn edit_text_file(
        &self,
        project: &ProjectRef,
        relative: &Path,
        expected: &str,
        replacement: &str,
    ) -> Result<TextEditResult> {
        self.edit_text_file_sync(project, relative, expected, replacement)
    }

    async fn apply_text_patch(
        &self,
        project: &ProjectRef,
        relative: &Path,
        hunks: &[TextPatchHunk],
    ) -> Result<TextPatchResult> {
        self.apply_text_patch_sync(project, relative, hunks)
    }

    async fn list_project_files(
        &self,
        project: &ProjectRef,
        max_entries: usize,
    ) -> Result<ProjectFileList> {
        self.list_project_files_sync(project, max_entries)
    }

    async fn search_project_text(
        &self,
        project: &ProjectRef,
        needle: &str,
        max_matches: usize,
    ) -> Result<ProjectTextSearchResult> {
        self.search_project_text_sync(project, needle, max_matches)
    }
}

fn canonical_existing_dir(path: &Path, error_code: &'static str) -> Result<PathBuf> {
    let metadata = fs::metadata(path).map_err(|_| workspace_error(error_code))?;
    if !metadata.is_dir() {
        return Err(workspace_error(error_code));
    }
    fs::canonicalize(path).map_err(|_| workspace_error(error_code))
}

fn reject_symlink_if_present(path: &Path, error_code: &'static str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(workspace_error(error_code));
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(_) => return Err(workspace_error(error_code)),
    }
    Ok(())
}

fn create_or_verify_fixed_child(
    parent: &Path,
    name: &'static str,
    error_code: &'static str,
) -> Result<PathBuf> {
    let child = parent.join(name);
    match fs::symlink_metadata(&child) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(workspace_error(error_code));
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => match fs::create_dir(&child) {
            Ok(()) => {}
            Err(create_error) if create_error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err(workspace_error(error_code)),
        },
        Err(_) => return Err(workspace_error(error_code)),
    }

    reject_symlink_if_present(&child, error_code)?;
    let canonical = canonical_existing_dir(&child, error_code)?;
    if canonical.parent() != Some(parent) {
        return Err(workspace_error(error_code));
    }
    Ok(canonical)
}

fn validate_relative_path(relative: &Path) -> Result<Vec<&OsStr>> {
    if relative.is_absolute() {
        return Err(workspace_error("project_path_must_be_relative"));
    }

    let text = relative
        .to_str()
        .ok_or_else(|| workspace_error("project_path_must_be_utf8"))?;
    if text.len() > MAX_RELATIVE_PATH_BYTES {
        return Err(workspace_error("project_path_too_long"));
    }
    if text.chars().any(char::is_control) || text.contains('\\') {
        return Err(workspace_error("project_path_contains_forbidden_character"));
    }

    let mut output = Vec::new();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => {
                let value_text = value
                    .to_str()
                    .ok_or_else(|| workspace_error("project_path_must_be_utf8"))?;
                if value_text.is_empty() || value_text.len() > MAX_COMPONENT_BYTES {
                    return Err(workspace_error("project_path_component_invalid"));
                }
                if value_text.starts_with(INTERNAL_TEMP_PREFIX) {
                    return Err(workspace_error("project_path_reserved_internal_name"));
                }
                output.push(value);
            }
            Component::ParentDir => return Err(workspace_error("project_path_parent_forbidden")),
            Component::RootDir | Component::Prefix(_) => {
                return Err(workspace_error("project_path_must_be_relative"));
            }
        }
    }

    Ok(output)
}

fn workspace_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Workspace(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "vibecoder-workspace-test-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("create test root");
            Self(root)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn runtime() -> (TestRoot, LocalWorkspaceRuntime) {
        let root = TestRoot::new();
        let runtime = LocalWorkspaceRuntime::initialize(LocalWorkspaceConfig {
            app_private_dir: root.0.clone(),
        })
        .expect("initialize runtime");
        (root, runtime)
    }

    #[test]
    fn creates_only_runtime_selected_project_root() {
        let (_root, runtime) = runtime();
        let spec = WorkspaceSpec::fresh();
        let id = spec.id();
        let project = runtime.create_project_sync(spec).expect("create project");
        assert_eq!(
            project.root,
            runtime.projects_root().join(id.0.hyphenated().to_string())
        );
        assert!(project.root.is_dir());
    }

    #[test]
    fn rejects_tampered_project_root() {
        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        let tampered = ProjectRef {
            id: project.id,
            root: runtime.app_private_root().to_path_buf(),
        };
        assert!(runtime.verify_project_sync(&tampered).is_err());
    }

    #[test]
    fn rejects_absolute_and_parent_paths() {
        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        assert!(
            runtime
                .resolve_project_path_sync(&project, Path::new("../escape"))
                .is_err()
        );
        assert!(
            runtime
                .resolve_project_path_sync(&project, Path::new("/absolute"))
                .is_err()
        );
    }

    #[test]
    fn resolves_safe_nonexistent_descendant() {
        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        let resolved = runtime
            .resolve_project_path_sync(&project, Path::new("src/new/file.rs"))
            .expect("resolve path");
        assert_eq!(resolved, project.root.join("src/new/file.rs"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_existing_symlink_components() {
        use std::os::unix::fs::symlink;

        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        let outside = TestRoot::new();
        symlink(&outside.0, project.root.join("escape")).expect("create symlink");
        assert!(
            runtime
                .resolve_project_path_sync(&project, Path::new("escape/file.txt"))
                .is_err()
        );
    }

    #[test]
    fn opens_existing_project_by_id_only() {
        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        let reopened = runtime.open_project_sync(project.id).expect("open project");
        assert_eq!(reopened, project);
    }

    #[test]
    fn rejects_control_and_backslash_paths() {
        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        assert!(
            runtime
                .resolve_project_path_sync(&project, Path::new("src\nspoof.rs"))
                .is_err()
        );
        assert!(
            runtime
                .resolve_project_path_sync(&project, Path::new("src\\escape.rs"))
                .is_err()
        );
    }

    #[test]
    fn rejects_non_directory_intermediate_component() {
        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        fs::write(project.root.join("src"), b"not a directory").expect("write fixture");
        assert!(
            runtime
                .resolve_project_path_sync(&project, Path::new("src/main.rs"))
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_fixed_product_root() {
        use std::os::unix::fs::symlink;

        let root = TestRoot::new();
        let outside = TestRoot::new();
        symlink(&outside.0, root.0.join(PRODUCT_ROOT_NAME)).expect("create product-root symlink");
        assert!(
            LocalWorkspaceRuntime::initialize(LocalWorkspaceConfig {
                app_private_dir: root.0.clone(),
            })
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn project_removal_does_not_follow_inner_symlink() {
        use std::os::unix::fs::symlink;

        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        let outside = TestRoot::new();
        let marker = outside.0.join("keep.txt");
        fs::write(&marker, b"keep").expect("write marker");
        symlink(&outside.0, project.root.join("outside-link")).expect("create symlink");
        runtime
            .remove_project_sync(&project)
            .expect("remove project");
        assert!(marker.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn safe_file_round_trip_uses_nested_directory() {
        use std::os::unix::fs::PermissionsExt;

        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        runtime
            .create_dir_all_sync(&project, Path::new("src/generated"))
            .expect("create directory tree");
        runtime
            .atomic_write_file_sync(
                &project,
                Path::new("src/generated/main.rs"),
                b"fn main() {}\n",
            )
            .expect("atomic write");
        let data = runtime
            .read_file_sync(&project, Path::new("src/generated/main.rs"), 1024)
            .expect("read file");
        assert_eq!(data, b"fn main() {}\n");

        let dir_mode = fs::metadata(project.root.join("src/generated"))
            .expect("directory metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        let file_mode = fs::metadata(project.root.join("src/generated/main.rs"))
            .expect("file metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_replaces_without_temp_artifacts() {
        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        runtime
            .atomic_write_file_sync(&project, Path::new("state.txt"), b"first")
            .expect("first write");
        runtime
            .atomic_write_file_sync(&project, Path::new("state.txt"), b"second")
            .expect("second write");
        assert_eq!(
            runtime
                .read_file_sync(&project, Path::new("state.txt"), 1024)
                .expect("read replacement"),
            b"second"
        );
        let temp_count = fs::read_dir(&project.root)
            .expect("read project root")
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vibecoder-tmp-")
            })
            .count();
        assert_eq!(temp_count, 0);
    }

    #[cfg(unix)]
    #[test]
    fn file_limits_fail_closed() {
        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        runtime
            .atomic_write_file_sync(&project, Path::new("small.txt"), b"1234")
            .expect("write fixture");
        assert!(
            runtime
                .read_file_sync(&project, Path::new("small.txt"), 3)
                .is_err()
        );
        assert!(
            runtime
                .read_file_sync(&project, Path::new("small.txt"), MAX_FILE_READ_BYTES + 1)
                .is_err()
        );
        let oversized = vec![0u8; MAX_FILE_WRITE_BYTES + 1];
        assert!(
            runtime
                .atomic_write_file_sync(&project, Path::new("too-large.bin"), &oversized)
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn read_and_write_reject_final_symlink() {
        use std::os::unix::fs::symlink;

        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        let outside = TestRoot::new();
        let outside_file = outside.0.join("secret.txt");
        fs::write(&outside_file, b"outside").expect("write outside file");
        symlink(&outside_file, project.root.join("alias.txt")).expect("create symlink");

        assert!(
            runtime
                .read_file_sync(&project, Path::new("alias.txt"), 1024)
                .is_err()
        );
        assert!(
            runtime
                .atomic_write_file_sync(&project, Path::new("alias.txt"), b"replace")
                .is_err()
        );
        assert_eq!(fs::read(&outside_file).expect("read outside"), b"outside");
    }

    #[cfg(unix)]
    #[test]
    fn read_and_write_reject_hard_link_alias() {
        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        let outside = TestRoot::new();
        let outside_file = outside.0.join("secret.txt");
        fs::write(&outside_file, b"outside").expect("write outside file");
        fs::hard_link(&outside_file, project.root.join("alias.txt")).expect("create hard link");

        assert!(
            runtime
                .read_file_sync(&project, Path::new("alias.txt"), 1024)
                .is_err()
        );
        assert!(
            runtime
                .atomic_write_file_sync(&project, Path::new("alias.txt"), b"replace")
                .is_err()
        );
        assert_eq!(fs::read(&outside_file).expect("read outside"), b"outside");
    }

    #[cfg(unix)]
    #[test]
    fn file_io_rejects_symlinked_parent_component() {
        use std::os::unix::fs::symlink;

        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        let outside = TestRoot::new();
        fs::write(outside.0.join("secret.txt"), b"outside").expect("write outside file");
        symlink(&outside.0, project.root.join("escape")).expect("create parent symlink");

        assert!(
            runtime
                .read_file_sync(&project, Path::new("escape/secret.txt"), 1024)
                .is_err()
        );
        assert!(
            runtime
                .atomic_write_file_sync(&project, Path::new("escape/new.txt"), b"bad")
                .is_err()
        );
        assert!(
            runtime
                .create_dir_all_sync(&project, Path::new("escape/new-dir"))
                .is_err()
        );
        assert!(!outside.0.join("new.txt").exists());
        assert!(!outside.0.join("new-dir").exists());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_requires_existing_parent() {
        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        assert!(
            runtime
                .atomic_write_file_sync(&project, Path::new("missing/file.txt"), b"data")
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_owner_execute_without_group_other_bits() {
        use std::os::unix::fs::PermissionsExt;

        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        let script = project.root.join("build.sh");
        fs::write(&script, b"#!/bin/sh\nexit 0\n").expect("write script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod fixture");
        runtime
            .atomic_write_file_sync(&project, Path::new("build.sh"), b"#!/bin/sh\nexit 1\n")
            .expect("replace executable");
        let mode = fs::metadata(&script)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_respects_owner_read_only_file() {
        use std::os::unix::fs::PermissionsExt;

        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        let file = project.root.join("readonly.txt");
        fs::write(&file, b"keep").expect("write fixture");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o400)).expect("chmod fixture");
        assert!(
            runtime
                .atomic_write_file_sync(&project, Path::new("readonly.txt"), b"replace")
                .is_err()
        );
        assert_eq!(fs::read(&file).expect("read fixture"), b"keep");
    }

    #[test]
    fn rejects_reserved_atomic_temp_namespace() {
        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        assert!(
            runtime
                .resolve_project_path_sync(&project, Path::new(".vibecoder-tmp-manual"))
                .is_err()
        );
        assert!(
            runtime
                .atomic_write_file_sync(&project, Path::new(".vibecoder-tmp-manual"), b"bad")
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn file_io_rejects_fifo_special_file() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        let fifo_path = project.root.join("tool.pipe");
        let fifo_path = CString::new(fifo_path.as_os_str().as_bytes()).expect("fifo path");
        // SAFETY: `fifo_path` is a valid NUL-terminated path and the mode is owner-only.
        let created = unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) };
        assert_eq!(
            created,
            0,
            "create fifo: {}",
            std::io::Error::last_os_error()
        );
        assert!(
            runtime
                .read_file_sync(&project, Path::new("tool.pipe"), 1024)
                .is_err()
        );
        assert!(
            runtime
                .atomic_write_file_sync(&project, Path::new("tool.pipe"), b"bad")
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn exact_text_edit_requires_one_unique_match() {
        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        runtime
            .atomic_write_file_sync(&project, Path::new("main.rs"), b"fn value() -> i32 { 1 }\n")
            .expect("write fixture");
        let result = runtime
            .edit_text_file_sync(&project, Path::new("main.rs"), "{ 1 }", "{ 2 }")
            .expect("edit unique text");
        assert_eq!(result.replacements, 1);
        assert_eq!(
            runtime
                .read_file_sync(&project, Path::new("main.rs"), 1024)
                .expect("read edited file"),
            b"fn value() -> i32 { 2 }\n"
        );

        runtime
            .atomic_write_file_sync(&project, Path::new("ambiguous.txt"), b"same same")
            .expect("write ambiguous fixture");
        assert!(
            runtime
                .edit_text_file_sync(&project, Path::new("ambiguous.txt"), "same", "changed",)
                .is_err()
        );
        assert_eq!(
            runtime
                .read_file_sync(&project, Path::new("ambiguous.txt"), 1024)
                .expect("read unchanged ambiguous fixture"),
            b"same same"
        );

        runtime
            .atomic_write_file_sync(&project, Path::new("overlap.txt"), b"aaa")
            .expect("write overlap fixture");
        assert!(
            runtime
                .edit_text_file_sync(&project, Path::new("overlap.txt"), "aa", "b")
                .is_err()
        );
        assert_eq!(
            runtime
                .read_file_sync(&project, Path::new("overlap.txt"), 1024)
                .expect("read unchanged overlap fixture"),
            b"aaa"
        );
    }

    #[cfg(unix)]
    #[test]
    fn text_edit_rejects_binary_and_preserves_executable_mode() {
        use std::os::unix::fs::PermissionsExt;

        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        runtime
            .atomic_write_file_sync(&project, Path::new("binary.bin"), &[0xff, 0xfe, 0xfd])
            .expect("write binary fixture");
        assert!(
            runtime
                .edit_text_file_sync(&project, Path::new("binary.bin"), "x", "y")
                .is_err()
        );

        let script = project.root.join("run.sh");
        fs::write(&script, b"#!/bin/sh\nexit 0\n").expect("write script fixture");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod script");
        runtime
            .edit_text_file_sync(&project, Path::new("run.sh"), "exit 0", "exit 1")
            .expect("edit executable");
        let mode = fs::metadata(&script)
            .expect("script metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn multi_hunk_patch_is_all_or_nothing() {
        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        runtime
            .atomic_write_file_sync(&project, Path::new("config.txt"), b"alpha=1\nbeta=2\n")
            .expect("write patch fixture");

        let good = vec![
            TextPatchHunk {
                expected: "alpha=1".into(),
                replacement: "alpha=10".into(),
            },
            TextPatchHunk {
                expected: "beta=2".into(),
                replacement: "beta=20".into(),
            },
        ];
        let result = runtime
            .apply_text_patch_sync(&project, Path::new("config.txt"), &good)
            .expect("apply patch");
        assert_eq!(result.hunks_applied, 2);
        assert_eq!(
            runtime
                .read_file_sync(&project, Path::new("config.txt"), 1024)
                .expect("read patched file"),
            b"alpha=10\nbeta=20\n"
        );

        let bad = vec![
            TextPatchHunk {
                expected: "alpha=10".into(),
                replacement: "alpha=100".into(),
            },
            TextPatchHunk {
                expected: "missing".into(),
                replacement: "never".into(),
            },
        ];
        assert!(
            runtime
                .apply_text_patch_sync(&project, Path::new("config.txt"), &bad)
                .is_err()
        );
        assert_eq!(
            runtime
                .read_file_sync(&project, Path::new("config.txt"), 1024)
                .expect("read after rejected patch"),
            b"alpha=10\nbeta=20\n"
        );
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    #[test]
    fn project_listing_is_sorted_and_skips_unsafe_aliases() {
        use std::os::unix::fs::symlink;

        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        runtime
            .create_dir_all_sync(&project, Path::new("src"))
            .expect("create src");
        runtime
            .atomic_write_file_sync(&project, Path::new("z.txt"), b"z")
            .expect("write z");
        runtime
            .atomic_write_file_sync(&project, Path::new("src/a.txt"), b"a")
            .expect("write a");
        fs::write(project.root.join(".vibecoder-tmp-leftover"), b"internal")
            .expect("write internal leftover");

        let outside = TestRoot::new();
        let outside_file = outside.0.join("secret.txt");
        fs::write(&outside_file, b"outside-secret").expect("write outside fixture");
        symlink(&outside_file, project.root.join("symlink.txt")).expect("create symlink");
        fs::hard_link(&outside_file, project.root.join("hardlink.txt")).expect("create hard link");

        let listed = runtime
            .list_project_files_sync(&project, 32)
            .expect("list project files");
        let names: Vec<_> = listed
            .files
            .iter()
            .map(|entry| entry.relative_path.as_str())
            .collect();
        assert_eq!(names, vec!["src/a.txt", "z.txt"]);
        assert!(listed.skipped_entries >= 3);
        assert!(!listed.truncated);

        let leaked = runtime
            .search_project_text_sync(&project, "outside-secret", 16)
            .expect("search must stay inside safe regular files");
        assert!(leaked.matches.is_empty());
        assert!(leaked.files_skipped >= 3);
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    #[test]
    fn literal_project_search_reports_bounded_line_and_column() {
        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        runtime
            .create_dir_all_sync(&project, Path::new("src"))
            .expect("create src");
        runtime
            .atomic_write_file_sync(
                &project,
                Path::new("src/main.rs"),
                b"first line\nlet needle = 1;\nlast line\n",
            )
            .expect("write source fixture");
        runtime
            .atomic_write_file_sync(&project, Path::new("binary.bin"), &[0xff, 0x00, 0xfe])
            .expect("write binary fixture");

        let result = runtime
            .search_project_text_sync(&project, "needle", 16)
            .expect("search project");
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].relative_path, "src/main.rs");
        assert_eq!(result.matches[0].line, 2);
        assert_eq!(result.matches[0].column, 5);
        assert!(result.matches[0].preview.contains("let needle = 1;"));
        assert!(result.files_skipped >= 1);
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    #[test]
    fn listing_and_search_bounds_fail_or_truncate_cleanly() {
        let (_root, runtime) = runtime();
        let project = runtime
            .create_project_sync(WorkspaceSpec::fresh())
            .expect("create project");
        runtime
            .atomic_write_file_sync(&project, Path::new("a.txt"), b"needle needle")
            .expect("write a");
        runtime
            .atomic_write_file_sync(&project, Path::new("b.txt"), b"needle")
            .expect("write b");

        assert!(runtime.list_project_files_sync(&project, 0).is_err());
        let one = runtime
            .list_project_files_sync(&project, 1)
            .expect("bounded list");
        assert_eq!(one.files.len(), 1);
        assert!(one.truncated);

        let search = runtime
            .search_project_text_sync(&project, "needle", 1)
            .expect("bounded search");
        assert_eq!(search.matches.len(), 1);
        assert!(search.truncated);
        assert!(runtime.search_project_text_sync(&project, "", 10).is_err());
    }
}
