//! Contracts for phone-local project workspaces and later build execution.
//!
//! The workspace runtime, not a caller/model, chooses the physical project root. Callers provide a
//! project identity only. File operations accept project-relative paths and must enforce containment
//! again at operation time; a previously resolved `PathBuf` is never an authorization token.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use vibecoder_domain::{ProjectId, ProjectRef, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceSpec {
    id: ProjectId,
}

impl WorkspaceSpec {
    pub fn fresh() -> Self {
        Self {
            id: ProjectId::new(),
        }
    }

    pub const fn id(&self) -> ProjectId {
        self.id
    }
}

impl Default for WorkspaceSpec {
    fn default() -> Self {
        Self::fresh()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceCapabilities {
    pub read_write_files: bool,
    pub managed_project_roots: bool,
    pub canonical_path_containment: bool,
    pub text_edit: bool,
    pub project_search: bool,
    pub commands: bool,
    pub process_isolation: bool,
    pub resource_limits: bool,
    pub snapshots: bool,
    /// Maximum bytes returned by one safe file-read operation. Zero means file I/O unavailable.
    pub max_file_read_bytes: u64,
    /// Maximum bytes accepted by one atomic file-write operation. Zero means file I/O unavailable.
    pub max_file_write_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextPatchHunk {
    pub expected: String,
    pub replacement: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextPatchResult {
    pub hunks_applied: u32,
    pub bytes_before: u64,
    pub bytes_after: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextEditResult {
    pub replacements: u32,
    pub bytes_before: u64,
    pub bytes_after: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectFileEntry {
    /// UTF-8 project-relative path. Absolute app-private paths are never exposed here.
    pub relative_path: String,
    pub size_bytes: u64,
    pub owner_executable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectFileList {
    pub files: Vec<ProjectFileEntry>,
    pub skipped_entries: u32,
    /// True when a caller/runtime bound stopped traversal before the complete project was listed.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectTextMatch {
    pub relative_path: String,
    /// One-based line number.
    pub line: u32,
    /// One-based Unicode-scalar column within the line.
    pub column: u32,
    /// Bounded single-line preview with control characters removed.
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectTextSearchResult {
    pub matches: Vec<ProjectTextMatch>,
    pub files_scanned: u32,
    pub files_skipped: u32,
    pub bytes_scanned: u64,
    /// True when a result/file/byte bound stopped traversal before the complete project was searched.
    pub truncated: bool,
}

#[async_trait]
pub trait WorkspaceRuntime: Send + Sync {
    fn capabilities(&self) -> WorkspaceCapabilities;

    /// Create a new project directory selected by the workspace runtime from the supplied id.
    async fn create_project(&self, spec: WorkspaceSpec) -> Result<ProjectRef>;

    /// Re-open one existing managed project by identity; no caller-controlled root is accepted.
    async fn open_project(&self, id: ProjectId) -> Result<ProjectRef>;

    async fn remove_project(&self, project: &ProjectRef) -> Result<()>;

    /// Verify that a serialized/caller-held `ProjectRef` still points at the exact managed root for
    /// its project id and that the root has not become a symlink or escaped containment.
    async fn verify_project(&self, project: &ProjectRef) -> Result<()>;

    /// Resolve one project-relative path beneath the verified project root for display/diagnostics.
    /// This does not authorize later I/O. File operations below re-open from the verified project
    /// directory and re-check every traversed component at operation time.
    async fn resolve_project_path(&self, project: &ProjectRef, relative: &Path) -> Result<PathBuf>;

    /// Create a project-relative directory tree. The local Android/Unix implementation walks from
    /// a verified project-directory handle and refuses symlink/non-directory traversal.
    async fn create_dir_all(&self, project: &ProjectRef, relative: &Path) -> Result<()>;

    /// Read one regular project file with an explicit caller limit. The runtime also applies its
    /// own hard maximum and rejects symlinks, special files, and suspicious hard-link aliases.
    async fn read_file(
        &self,
        project: &ProjectRef,
        relative: &Path,
        max_bytes: usize,
    ) -> Result<Vec<u8>>;

    /// Return whether one project-relative entry currently exists as a regular single-link file.
    /// Symlinks, special files, and suspicious hard-link aliases are rejected rather than reported
    /// as ordinary files. This is an operation-time check, not durable authorization.
    async fn regular_file_exists(&self, project: &ProjectRef, relative: &Path) -> Result<bool>;

    /// Replace/create one regular project file atomically within its existing parent directory.
    /// This API never truncates an existing inode in place. Parent directories must already exist.
    async fn atomic_write_file(
        &self,
        project: &ProjectRef,
        relative: &Path,
        contents: &[u8],
    ) -> Result<()>;

    /// Atomically replace exactly one occurrence of `expected` in a UTF-8 regular file. If the
    /// expected text is absent, appears more than once, the target changes during the operation,
    /// or the replacement would exceed the write ceiling, no replacement is committed.
    async fn edit_text_file(
        &self,
        project: &ProjectRef,
        relative: &Path,
        expected: &str,
        replacement: &str,
    ) -> Result<TextEditResult>;

    /// Apply a bounded sequence of exact-match UTF-8 hunks as one all-or-nothing atomic patch.
    /// Every hunk must have exactly one match in the evolving in-memory file; any failure commits
    /// zero hunks.
    async fn apply_text_patch(
        &self,
        project: &ProjectRef,
        relative: &Path,
        hunks: &[TextPatchHunk],
    ) -> Result<TextPatchResult>;

    /// Discover regular, single-link project files without following symlinks. Traversal is
    /// deterministic and bounded by `max_entries`; internal VibeCoder temporary files are hidden.
    async fn list_project_files(
        &self,
        project: &ProjectRef,
        max_entries: usize,
    ) -> Result<ProjectFileList>;

    /// Literal UTF-8 text search over safely discovered project files. Binary/non-UTF8, oversized,
    /// special, symlink, and hard-linked files are skipped rather than followed.
    async fn search_project_text(
        &self,
        project: &ProjectRef,
        needle: &str,
        max_matches: usize,
    ) -> Result<ProjectTextSearchResult>;
}
