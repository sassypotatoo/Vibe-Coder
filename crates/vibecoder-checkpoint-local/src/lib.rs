//! Android/Linux app-private checkpoint store.
//!
//! Checkpoints are immutable trees stored outside agent-visible project roots. Creation publishes
//! only after source/copy/source digests agree. Rollback clones the immutable snapshot into a
//! reserved sibling staging directory and atomically exchanges it with the live project using
//! `renameat2(RENAME_EXCHANGE)`; unsafe multi-rename fallbacks are deliberately not used.

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use vibecoder_checkpoint_contract::{
    CHECKPOINT_SCHEMA_V1, CheckpointCapabilities, CheckpointId, CheckpointMetadata,
    CheckpointReason, CheckpointStore, MAX_CHECKPOINT_DEPTH, MAX_CHECKPOINT_FILES,
    MAX_CHECKPOINT_TOTAL_BYTES, MAX_CHECKPOINTS_PER_PROJECT, RollbackResult, checkpoint_error,
};
use vibecoder_domain::{ProjectId, ProjectRef, Result};

const PRODUCT_ROOT_NAME: &str = "vibecoder";
const PROJECTS_ROOT_NAME: &str = "projects";
const CHECKPOINTS_ROOT_NAME: &str = "checkpoints";
const SNAPSHOT_TREE_NAME: &str = "tree";
const METADATA_NAME: &str = "metadata.json";
const CHECKPOINT_TEMP_PREFIX: &str = ".vibecoder-checkpoint-tmp-";
const ROLLBACK_TEMP_PREFIX: &str = ".vibecoder-rollback-";
const WORKSPACE_TEMP_PREFIX: &str = ".vibecoder-tmp-";
const PRIVATE_DIR_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
const MAX_METADATA_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalCheckpointConfig {
    /// Existing application-private directory supplied by Android/platform code.
    pub app_private_dir: PathBuf,
}

#[derive(Debug)]
pub struct LocalCheckpointStore {
    projects_root: PathBuf,
    checkpoints_root: PathBuf,
    gate: Mutex<()>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeDigest {
    file_count: u64,
    total_bytes: u64,
    sha256: String,
}

impl LocalCheckpointStore {
    pub fn initialize(config: LocalCheckpointConfig) -> Result<Self> {
        if !config.app_private_dir.is_absolute() {
            return Err(checkpoint_error("checkpoint_app_private_root_not_absolute"));
        }
        reject_symlink(
            &config.app_private_dir,
            "checkpoint_app_private_root_is_symlink",
        )?;
        let app_private_root = canonical_private_dir(&config.app_private_dir)?;
        let product_root = create_or_verify_private_child(&app_private_root, PRODUCT_ROOT_NAME)?;
        let projects_root = create_or_verify_private_child(&product_root, PROJECTS_ROOT_NAME)?;
        let checkpoints_root =
            create_or_verify_private_child(&product_root, CHECKPOINTS_ROOT_NAME)?;
        if !projects_root.starts_with(&app_private_root)
            || !checkpoints_root.starts_with(&app_private_root)
        {
            return Err(checkpoint_error("checkpoint_root_escaped_app_private_root"));
        }
        let store = Self {
            projects_root,
            checkpoints_root,
            gate: Mutex::new(()),
        };
        store.cleanup_rollback_staging()?;
        store.cleanup_checkpoint_temps()?;
        Ok(store)
    }

    pub fn checkpoint_root(&self) -> &Path {
        &self.checkpoints_root
    }

    fn lock_gate(&self) -> Result<MutexGuard<'_, ()>> {
        self.gate
            .lock()
            .map_err(|_| checkpoint_error("checkpoint_gate_poisoned"))
    }

    fn expected_project_root(&self, id: ProjectId) -> PathBuf {
        self.projects_root.join(id.0.hyphenated().to_string())
    }

    fn verify_project(&self, project: &ProjectRef) -> Result<()> {
        let expected = self.expected_project_root(project.id);
        if project.root != expected || !project.root.is_absolute() {
            return Err(checkpoint_error("checkpoint_project_root_mismatch"));
        }
        let metadata = fs::symlink_metadata(&project.root)
            .map_err(|_| checkpoint_error("checkpoint_project_root_unavailable"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(checkpoint_error("checkpoint_project_root_invalid"));
        }
        let canonical = fs::canonicalize(&project.root)
            .map_err(|_| checkpoint_error("checkpoint_project_root_unavailable"))?;
        if canonical != expected || !canonical.starts_with(&self.projects_root) {
            return Err(checkpoint_error("checkpoint_project_root_not_contained"));
        }
        Ok(())
    }

    fn cleanup_rollback_staging(&self) -> Result<()> {
        let entries = fs::read_dir(&self.projects_root)
            .map_err(|_| checkpoint_error("checkpoint_projects_list_failed"))?;
        for entry in entries {
            let entry = entry.map_err(|_| checkpoint_error("checkpoint_projects_list_failed"))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.starts_with(ROLLBACK_TEMP_PREFIX) {
                continue;
            }
            let suffix = &name[ROLLBACK_TEMP_PREFIX.len()..];
            let Ok(uuid) = Uuid::parse_str(suffix) else {
                continue;
            };
            if uuid.hyphenated().to_string() != suffix {
                continue;
            }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|_| checkpoint_error("checkpoint_rollback_cleanup_metadata_failed"))?;
            if metadata.file_type().is_symlink() {
                fs::remove_file(&path)
                    .map_err(|_| checkpoint_error("checkpoint_rollback_cleanup_failed"))?;
            } else if metadata.is_dir() {
                fs::remove_dir_all(&path)
                    .map_err(|_| checkpoint_error("checkpoint_rollback_cleanup_failed"))?;
            } else {
                return Err(checkpoint_error("checkpoint_rollback_staging_invalid"));
            }
        }
        Ok(())
    }

    fn cleanup_checkpoint_temps(&self) -> Result<()> {
        for project_entry in fs::read_dir(&self.checkpoints_root)
            .map_err(|_| checkpoint_error("checkpoint_cleanup_list_failed"))?
        {
            let project_entry =
                project_entry.map_err(|_| checkpoint_error("checkpoint_cleanup_list_failed"))?;
            let project_meta = fs::symlink_metadata(project_entry.path())
                .map_err(|_| checkpoint_error("checkpoint_cleanup_metadata_failed"))?;
            if project_meta.file_type().is_symlink() || !project_meta.is_dir() {
                return Err(checkpoint_error("checkpoint_project_directory_invalid"));
            }
            for entry in fs::read_dir(project_entry.path())
                .map_err(|_| checkpoint_error("checkpoint_cleanup_list_failed"))?
            {
                let entry =
                    entry.map_err(|_| checkpoint_error("checkpoint_cleanup_list_failed"))?;
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if !name.starts_with(CHECKPOINT_TEMP_PREFIX) {
                    continue;
                }
                let suffix = &name[CHECKPOINT_TEMP_PREFIX.len()..];
                let Ok(uuid) = Uuid::parse_str(suffix) else {
                    continue;
                };
                if uuid.hyphenated().to_string() != suffix {
                    continue;
                }
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path)
                    .map_err(|_| checkpoint_error("checkpoint_cleanup_metadata_failed"))?;
                if metadata.file_type().is_symlink() {
                    fs::remove_file(&path)
                        .map_err(|_| checkpoint_error("checkpoint_temp_cleanup_failed"))?;
                } else if metadata.is_dir() {
                    fs::remove_dir_all(&path)
                        .map_err(|_| checkpoint_error("checkpoint_temp_cleanup_failed"))?;
                } else {
                    return Err(checkpoint_error("checkpoint_temp_staging_invalid"));
                }
            }
        }
        Ok(())
    }

    fn project_checkpoint_dir(&self, project_id: ProjectId) -> PathBuf {
        self.checkpoints_root
            .join(project_id.0.hyphenated().to_string())
    }

    fn checkpoint_dir(&self, project_id: ProjectId, checkpoint_id: CheckpointId) -> PathBuf {
        self.project_checkpoint_dir(project_id)
            .join(checkpoint_id.0.hyphenated().to_string())
    }

    fn load_metadata_sync(
        &self,
        project_id: ProjectId,
        checkpoint_id: CheckpointId,
    ) -> Result<Option<CheckpointMetadata>> {
        let dir = self.checkpoint_dir(project_id, checkpoint_id);
        match fs::symlink_metadata(&dir) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(checkpoint_error("checkpoint_directory_invalid"));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(checkpoint_error("checkpoint_directory_metadata_failed")),
        }
        let path = dir.join(METADATA_NAME);
        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let file = options
            .open(&path)
            .map_err(|_| checkpoint_error("checkpoint_metadata_open_failed"))?;
        let metadata = file
            .metadata()
            .map_err(|_| checkpoint_error("checkpoint_metadata_stat_failed"))?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.len() > MAX_METADATA_BYTES as u64
        {
            return Err(checkpoint_error("checkpoint_metadata_invalid"));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(MAX_METADATA_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| checkpoint_error("checkpoint_metadata_read_failed"))?;
        if bytes.len() > MAX_METADATA_BYTES {
            return Err(checkpoint_error("checkpoint_metadata_too_large"));
        }
        let parsed: CheckpointMetadata = serde_json::from_slice(&bytes)
            .map_err(|_| checkpoint_error("checkpoint_metadata_json_invalid"))?;
        parsed.validate()?;
        if parsed.project_id != project_id || parsed.checkpoint_id != checkpoint_id {
            return Err(checkpoint_error("checkpoint_metadata_identity_mismatch"));
        }
        Ok(Some(parsed))
    }

    fn create_checkpoint_sync(
        &self,
        project: &ProjectRef,
        reason: CheckpointReason,
    ) -> Result<CheckpointMetadata> {
        self.verify_project(project)?;
        let _gate = self.lock_gate()?;
        let project_checkpoint_dir = create_or_verify_private_child(
            &self.checkpoints_root,
            &project.id.0.hyphenated().to_string(),
        )?;
        let existing = list_checkpoint_ids(&project_checkpoint_dir)?;
        if existing.len() >= MAX_CHECKPOINTS_PER_PROJECT {
            return Err(checkpoint_error("checkpoint_project_limit_reached"));
        }

        let checkpoint_id = fresh_checkpoint_id(&project_checkpoint_dir)?;
        let temp_name = format!("{CHECKPOINT_TEMP_PREFIX}{}", checkpoint_id.0.hyphenated());
        let temp_dir = project_checkpoint_dir.join(&temp_name);
        fs::create_dir(&temp_dir).map_err(|_| checkpoint_error("checkpoint_temp_create_failed"))?;
        set_private_dir_mode(&temp_dir)?;
        let final_dir = project_checkpoint_dir.join(checkpoint_id.0.hyphenated().to_string());
        let tree_dir = temp_dir.join(SNAPSHOT_TREE_NAME);
        let result = (|| {
            fs::create_dir(&tree_dir)
                .map_err(|_| checkpoint_error("checkpoint_tree_create_failed"))?;
            set_private_dir_mode(&tree_dir)?;

            let source_before = digest_tree(&project.root)?;
            let copied = copy_tree_and_digest(&project.root, &tree_dir)?;
            let copied_verify = digest_tree(&tree_dir)?;
            if copied != copied_verify {
                return Err(checkpoint_error("checkpoint_copy_integrity_mismatch"));
            }
            let source_after = digest_tree(&project.root)?;
            if source_before != copied || copied != source_after {
                return Err(checkpoint_error(
                    "checkpoint_source_changed_during_snapshot",
                ));
            }

            let created_unix_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| checkpoint_error("checkpoint_clock_invalid"))?
                .as_millis()
                .try_into()
                .map_err(|_| checkpoint_error("checkpoint_clock_overflow"))?;
            let metadata = CheckpointMetadata {
                schema: CHECKPOINT_SCHEMA_V1,
                checkpoint_id,
                project_id: project.id,
                created_unix_ms,
                reason,
                file_count: copied.file_count,
                total_bytes: copied.total_bytes,
                tree_sha256: copied.sha256.clone(),
            };
            metadata.validate()?;
            write_metadata(&temp_dir.join(METADATA_NAME), &metadata)?;
            sync_dir(&tree_dir)?;
            sync_dir(&temp_dir)?;
            fs::rename(&temp_dir, &final_dir)
                .map_err(|_| checkpoint_error("checkpoint_publish_rename_failed"))?;
            sync_dir(&project_checkpoint_dir)?;
            Ok(metadata)
        })();
        if result.is_err() && temp_dir.exists() {
            let _ = fs::remove_dir_all(&temp_dir);
        }
        result
    }

    fn rollback_sync(
        &self,
        project: &ProjectRef,
        checkpoint_id: CheckpointId,
    ) -> Result<RollbackResult> {
        self.verify_project(project)?;
        let _gate = self.lock_gate()?;
        let metadata = self
            .load_metadata_sync(project.id, checkpoint_id)?
            .ok_or_else(|| checkpoint_error("checkpoint_not_found"))?;
        let snapshot_tree = self
            .checkpoint_dir(project.id, checkpoint_id)
            .join(SNAPSHOT_TREE_NAME);
        let snapshot_digest = digest_tree(&snapshot_tree)?;
        if snapshot_digest.sha256 != metadata.tree_sha256
            || snapshot_digest.file_count != metadata.file_count
            || snapshot_digest.total_bytes != metadata.total_bytes
        {
            return Err(checkpoint_error("checkpoint_integrity_verification_failed"));
        }

        #[cfg(not(any(target_os = "android", target_os = "linux")))]
        {
            let _ = snapshot_tree;
            return Err(checkpoint_error(
                "checkpoint_atomic_exchange_unsupported_platform",
            ));
        }

        #[cfg(any(target_os = "android", target_os = "linux"))]
        {
            let staging_name = format!("{ROLLBACK_TEMP_PREFIX}{}", Uuid::new_v4().hyphenated());
            let staging = self.projects_root.join(&staging_name);
            fs::create_dir(&staging)
                .map_err(|_| checkpoint_error("checkpoint_rollback_stage_create_failed"))?;
            set_private_dir_mode(&staging)?;
            let staged = match copy_tree_and_digest(&snapshot_tree, &staging) {
                Ok(value) => value,
                Err(error) => {
                    let _ = fs::remove_dir_all(&staging);
                    return Err(error);
                }
            };
            if staged != snapshot_digest || digest_tree(&staging)? != snapshot_digest {
                let _ = fs::remove_dir_all(&staging);
                return Err(checkpoint_error(
                    "checkpoint_rollback_stage_integrity_failed",
                ));
            }

            let project_name = project.id.0.hyphenated().to_string();
            exchange_children(&self.projects_root, &project_name, &staging_name)?;
            if sync_dir(&self.projects_root).is_err() {
                let recovery = exchange_children(&self.projects_root, &project_name, &staging_name)
                    .and_then(|_| sync_dir(&self.projects_root));
                if recovery.is_err() {
                    return Err(checkpoint_error("checkpoint_rollback_recovery_failed"));
                }
                let _ = fs::remove_dir_all(&staging);
                return Err(checkpoint_error("checkpoint_rollback_exchange_sync_failed"));
            }

            let restored = digest_tree(&project.root);
            if restored.as_ref().ok() != Some(&snapshot_digest) {
                let recovery = exchange_children(&self.projects_root, &project_name, &staging_name)
                    .and_then(|_| sync_dir(&self.projects_root));
                if recovery.is_err() {
                    return Err(checkpoint_error("checkpoint_rollback_recovery_failed"));
                }
                let _ = fs::remove_dir_all(&staging);
                return Err(checkpoint_error(
                    "checkpoint_rollback_post_exchange_verify_failed",
                ));
            }

            // The live project is already atomically restored, parent-synced, and verified. Old-tree
            // cleanup must not turn a committed rollback into an ambiguous error. Initialization
            // removes any canonical rollback staging directory left by a failed cleanup.
            if fs::remove_dir_all(&staging).is_ok() {
                let _ = sync_dir(&self.projects_root);
            }
            Ok(RollbackResult {
                project_id: project.id,
                checkpoint_id,
                restored_file_count: metadata.file_count,
                restored_total_bytes: metadata.total_bytes,
                tree_sha256: metadata.tree_sha256,
            })
        }
    }
}

#[async_trait]
impl CheckpointStore for LocalCheckpointStore {
    fn capabilities(&self) -> CheckpointCapabilities {
        CheckpointCapabilities {
            immutable_snapshots: true,
            integrity_digest: true,
            rollback: cfg!(any(target_os = "android", target_os = "linux")),
            atomic_project_exchange: cfg!(any(target_os = "android", target_os = "linux")),
            secrets_indexed: false,
        }
    }

    async fn create_checkpoint(
        &self,
        project: &ProjectRef,
        reason: CheckpointReason,
    ) -> Result<CheckpointMetadata> {
        self.create_checkpoint_sync(project, reason)
    }

    async fn list_checkpoints(
        &self,
        project_id: ProjectId,
        max_results: usize,
    ) -> Result<Vec<CheckpointMetadata>> {
        if max_results == 0 || max_results > MAX_CHECKPOINTS_PER_PROJECT {
            return Err(checkpoint_error("checkpoint_list_limit_invalid"));
        }
        let _gate = self.lock_gate()?;
        let dir = self.project_checkpoint_dir(project_id);
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for id in list_checkpoint_ids(&dir)? {
            if let Some(metadata) = self.load_metadata_sync(project_id, id)? {
                out.push(metadata);
            }
        }
        out.sort_by(|a, b| {
            b.created_unix_ms.cmp(&a.created_unix_ms).then_with(|| {
                b.checkpoint_id
                    .0
                    .as_bytes()
                    .cmp(a.checkpoint_id.0.as_bytes())
            })
        });
        out.truncate(max_results);
        Ok(out)
    }

    async fn load_checkpoint(
        &self,
        project_id: ProjectId,
        checkpoint_id: CheckpointId,
    ) -> Result<Option<CheckpointMetadata>> {
        let _gate = self.lock_gate()?;
        self.load_metadata_sync(project_id, checkpoint_id)
    }

    async fn rollback_project(
        &self,
        project: &ProjectRef,
        checkpoint_id: CheckpointId,
    ) -> Result<RollbackResult> {
        self.rollback_sync(project, checkpoint_id)
    }

    async fn remove_checkpoint(
        &self,
        project_id: ProjectId,
        checkpoint_id: CheckpointId,
    ) -> Result<()> {
        let _gate = self.lock_gate()?;
        let dir = self.checkpoint_dir(project_id, checkpoint_id);
        let metadata =
            fs::symlink_metadata(&dir).map_err(|_| checkpoint_error("checkpoint_not_found"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(checkpoint_error("checkpoint_directory_invalid"));
        }
        fs::remove_dir_all(&dir).map_err(|_| checkpoint_error("checkpoint_remove_failed"))?;
        if let Some(parent) = dir.parent() {
            sync_dir(parent)?;
        }
        Ok(())
    }
}

fn copy_tree_and_digest(source: &Path, destination: &Path) -> Result<TreeDigest> {
    let mut state = DigestState::new();
    copy_dir_recursive(source, destination, "", 0, &mut state)?;
    Ok(state.finish())
}

fn digest_tree(root: &Path) -> Result<TreeDigest> {
    let mut state = DigestState::new();
    hash_dir_recursive(root, "", 0, &mut state)?;
    Ok(state.finish())
}

struct DigestState {
    hasher: Sha256,
    file_count: u64,
    total_bytes: u64,
}

impl DigestState {
    fn new() -> Self {
        Self {
            hasher: Sha256::new(),
            file_count: 0,
            total_bytes: 0,
        }
    }

    fn record_dir(&mut self, relative: &str) {
        self.hasher.update(b"D");
        record_path(&mut self.hasher, relative);
    }

    fn begin_file(&mut self, relative: &str, size: u64, executable: bool) -> Result<()> {
        self.file_count = self
            .file_count
            .checked_add(1)
            .ok_or_else(|| checkpoint_error("checkpoint_file_count_overflow"))?;
        if self.file_count > MAX_CHECKPOINT_FILES {
            return Err(checkpoint_error("checkpoint_file_count_limit"));
        }
        self.total_bytes = self
            .total_bytes
            .checked_add(size)
            .ok_or_else(|| checkpoint_error("checkpoint_total_bytes_overflow"))?;
        if self.total_bytes > MAX_CHECKPOINT_TOTAL_BYTES {
            return Err(checkpoint_error("checkpoint_total_bytes_limit"));
        }
        self.hasher.update(b"F");
        record_path(&mut self.hasher, relative);
        self.hasher.update(size.to_be_bytes());
        self.hasher.update([u8::from(executable)]);
        Ok(())
    }

    fn finish(self) -> TreeDigest {
        TreeDigest {
            file_count: self.file_count,
            total_bytes: self.total_bytes,
            sha256: format!("{:x}", self.hasher.finalize()),
        }
    }
}

fn record_path(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

struct PinnedDirectory {
    _file: File,
    proc_path: PathBuf,
    names: Vec<String>,
}

fn pin_directory(path: &Path) -> Result<PinnedDirectory> {
    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    {
        let _ = path;
        return Err(checkpoint_error(
            "checkpoint_secure_tree_walk_unsupported_platform",
        ));
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let file = options
            .open(path)
            .map_err(|_| checkpoint_error("checkpoint_tree_directory_open_failed"))?;
        let metadata = file
            .metadata()
            .map_err(|_| checkpoint_error("checkpoint_tree_directory_metadata_failed"))?;
        if !metadata.is_dir() {
            return Err(checkpoint_error("checkpoint_tree_directory_invalid"));
        }
        let proc_path = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()));
        let mut names = Vec::new();
        for entry in fs::read_dir(&proc_path)
            .map_err(|_| checkpoint_error("checkpoint_tree_read_dir_failed"))?
        {
            let entry = entry.map_err(|_| checkpoint_error("checkpoint_tree_read_dir_failed"))?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| checkpoint_error("checkpoint_path_not_utf8"))?;
            if name.is_empty()
                || name.chars().any(char::is_control)
                || name.contains('/')
                || name.contains('\\')
                || name.starts_with(WORKSPACE_TEMP_PREFIX)
                || name.starts_with(ROLLBACK_TEMP_PREFIX)
                || name.starts_with(CHECKPOINT_TEMP_PREFIX)
            {
                return Err(checkpoint_error("checkpoint_path_component_invalid"));
            }
            names.push(name.to_owned());
        }
        names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        Ok(PinnedDirectory {
            _file: file,
            proc_path,
            names,
        })
    }
}

fn copy_dir_recursive(
    source: &Path,
    destination: &Path,
    prefix: &str,
    depth: usize,
    state: &mut DigestState,
) -> Result<()> {
    if depth > MAX_CHECKPOINT_DEPTH {
        return Err(checkpoint_error("checkpoint_depth_limit"));
    }
    let source_dir = pin_directory(source)?;
    let destination_dir = pin_directory(destination)?;
    for name in &source_dir.names {
        let path = source_dir.proc_path.join(name);
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let before = fs::symlink_metadata(&path)
            .map_err(|_| checkpoint_error("checkpoint_tree_entry_metadata_failed"))?;
        if before.file_type().is_symlink() {
            return Err(checkpoint_error("checkpoint_symlink_forbidden"));
        }
        if before.is_dir() {
            state.record_dir(&relative);
            let dest = destination_dir.proc_path.join(name);
            fs::create_dir(&dest)
                .map_err(|_| checkpoint_error("checkpoint_copy_dir_create_failed"))?;
            set_private_dir_mode(&dest)?;
            copy_dir_recursive(&path, &dest, &relative, depth + 1, state)?;
            sync_dir(&dest)?;
        } else if before.is_file() {
            if before.nlink() != 1 {
                return Err(checkpoint_error("checkpoint_hard_link_forbidden"));
            }
            let executable = before.mode() & 0o100 != 0;
            state.begin_file(&relative, before.len(), executable)?;
            let mut options = OpenOptions::new();
            options
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
            let mut input = options
                .open(&path)
                .map_err(|_| checkpoint_error("checkpoint_source_file_open_failed"))?;
            let opened = input
                .metadata()
                .map_err(|_| checkpoint_error("checkpoint_source_file_stat_failed"))?;
            if !opened.is_file()
                || opened.nlink() != 1
                || opened.dev() != before.dev()
                || opened.ino() != before.ino()
            {
                return Err(checkpoint_error("checkpoint_source_changed_during_open"));
            }
            let mode = PRIVATE_FILE_MODE | if executable { 0o100 } else { 0 };
            let dest = destination_dir.proc_path.join(name);
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(mode)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&dest)
                .map_err(|_| checkpoint_error("checkpoint_copy_file_create_failed"))?;
            let mut remaining = before.len();
            let mut buffer = [0u8; 64 * 1024];
            while remaining > 0 {
                let want = buffer.len().min(remaining as usize);
                let read = input
                    .read(&mut buffer[..want])
                    .map_err(|_| checkpoint_error("checkpoint_source_file_read_failed"))?;
                if read == 0 {
                    return Err(checkpoint_error("checkpoint_source_file_short_read"));
                }
                state.hasher.update(&buffer[..read]);
                output
                    .write_all(&buffer[..read])
                    .map_err(|_| checkpoint_error("checkpoint_copy_file_write_failed"))?;
                remaining -= read as u64;
            }
            let mut extra = [0u8; 1];
            if input
                .read(&mut extra)
                .map_err(|_| checkpoint_error("checkpoint_source_file_read_failed"))?
                != 0
            {
                return Err(checkpoint_error("checkpoint_source_file_grew_during_copy"));
            }
            output
                .sync_all()
                .map_err(|_| checkpoint_error("checkpoint_copy_file_sync_failed"))?;
            let after = fs::symlink_metadata(&path)
                .map_err(|_| checkpoint_error("checkpoint_source_file_post_stat_failed"))?;
            if after.dev() != before.dev()
                || after.ino() != before.ino()
                || after.len() != before.len()
                || after.nlink() != 1
            {
                return Err(checkpoint_error("checkpoint_source_changed_during_copy"));
            }
        } else {
            return Err(checkpoint_error("checkpoint_special_file_forbidden"));
        }
    }
    Ok(())
}

fn hash_dir_recursive(
    root: &Path,
    prefix: &str,
    depth: usize,
    state: &mut DigestState,
) -> Result<()> {
    if depth > MAX_CHECKPOINT_DEPTH {
        return Err(checkpoint_error("checkpoint_depth_limit"));
    }
    let source_dir = pin_directory(root)?;
    for name in &source_dir.names {
        let path = source_dir.proc_path.join(name);
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let before = fs::symlink_metadata(&path)
            .map_err(|_| checkpoint_error("checkpoint_tree_entry_metadata_failed"))?;
        if before.file_type().is_symlink() {
            return Err(checkpoint_error("checkpoint_symlink_forbidden"));
        }
        if before.is_dir() {
            state.record_dir(&relative);
            hash_dir_recursive(&path, &relative, depth + 1, state)?;
        } else if before.is_file() {
            if before.nlink() != 1 {
                return Err(checkpoint_error("checkpoint_hard_link_forbidden"));
            }
            let executable = before.mode() & 0o100 != 0;
            state.begin_file(&relative, before.len(), executable)?;
            let mut options = OpenOptions::new();
            options
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
            let mut file = options
                .open(&path)
                .map_err(|_| checkpoint_error("checkpoint_hash_file_open_failed"))?;
            let opened = file
                .metadata()
                .map_err(|_| checkpoint_error("checkpoint_hash_file_stat_failed"))?;
            if opened.dev() != before.dev() || opened.ino() != before.ino() || opened.nlink() != 1 {
                return Err(checkpoint_error("checkpoint_hash_file_changed_during_open"));
            }
            let mut remaining = before.len();
            let mut buffer = [0u8; 64 * 1024];
            while remaining > 0 {
                let want = buffer.len().min(remaining as usize);
                let read = file
                    .read(&mut buffer[..want])
                    .map_err(|_| checkpoint_error("checkpoint_hash_file_read_failed"))?;
                if read == 0 {
                    return Err(checkpoint_error("checkpoint_hash_file_short_read"));
                }
                state.hasher.update(&buffer[..read]);
                remaining -= read as u64;
            }
            let mut extra = [0u8; 1];
            if file
                .read(&mut extra)
                .map_err(|_| checkpoint_error("checkpoint_hash_file_read_failed"))?
                != 0
            {
                return Err(checkpoint_error("checkpoint_hash_file_grew"));
            }
            let after = fs::symlink_metadata(&path)
                .map_err(|_| checkpoint_error("checkpoint_hash_file_post_stat_failed"))?;
            if after.dev() != before.dev()
                || after.ino() != before.ino()
                || after.len() != before.len()
                || after.nlink() != 1
            {
                return Err(checkpoint_error("checkpoint_hash_file_changed"));
            }
        } else {
            return Err(checkpoint_error("checkpoint_special_file_forbidden"));
        }
    }
    Ok(())
}

fn write_metadata(path: &Path, metadata: &CheckpointMetadata) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(metadata)
        .map_err(|_| checkpoint_error("checkpoint_metadata_serialize_failed"))?;
    if bytes.len() > MAX_METADATA_BYTES {
        return Err(checkpoint_error("checkpoint_metadata_too_large"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(PRIVATE_FILE_MODE)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| checkpoint_error("checkpoint_metadata_create_failed"))?;
    file.write_all(&bytes)
        .map_err(|_| checkpoint_error("checkpoint_metadata_write_failed"))?;
    file.sync_all()
        .map_err(|_| checkpoint_error("checkpoint_metadata_sync_failed"))
}

fn fresh_checkpoint_id(parent: &Path) -> Result<CheckpointId> {
    for _ in 0..8 {
        let id = CheckpointId::new();
        let final_path = parent.join(id.0.hyphenated().to_string());
        let temp_path = parent.join(format!("{CHECKPOINT_TEMP_PREFIX}{}", id.0.hyphenated()));
        if !final_path.exists() && !temp_path.exists() {
            return Ok(id);
        }
    }
    Err(checkpoint_error("checkpoint_id_collision"))
}

fn list_checkpoint_ids(parent: &Path) -> Result<Vec<CheckpointId>> {
    let metadata = fs::symlink_metadata(parent)
        .map_err(|_| checkpoint_error("checkpoint_project_directory_unavailable"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(checkpoint_error("checkpoint_project_directory_invalid"));
    }
    let mut ids = Vec::new();
    for entry in fs::read_dir(parent).map_err(|_| checkpoint_error("checkpoint_list_failed"))? {
        let entry = entry.map_err(|_| checkpoint_error("checkpoint_list_failed"))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err(checkpoint_error("checkpoint_directory_name_invalid"));
        };
        if name.starts_with(CHECKPOINT_TEMP_PREFIX) {
            continue;
        }
        let uuid = Uuid::parse_str(name)
            .map_err(|_| checkpoint_error("checkpoint_directory_name_invalid"))?;
        if uuid.hyphenated().to_string() != name {
            return Err(checkpoint_error("checkpoint_directory_name_noncanonical"));
        }
        let meta = fs::symlink_metadata(entry.path())
            .map_err(|_| checkpoint_error("checkpoint_directory_metadata_failed"))?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(checkpoint_error("checkpoint_directory_invalid"));
        }
        ids.push(CheckpointId(uuid));
        if ids.len() > MAX_CHECKPOINTS_PER_PROJECT {
            return Err(checkpoint_error("checkpoint_project_limit_exceeded"));
        }
    }
    ids.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    Ok(ids)
}

fn canonical_private_dir(path: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path)
        .map_err(|_| checkpoint_error("checkpoint_app_private_root_unavailable"))?;
    let metadata = fs::metadata(&canonical)
        .map_err(|_| checkpoint_error("checkpoint_app_private_root_unavailable"))?;
    if !metadata.is_dir() {
        return Err(checkpoint_error("checkpoint_app_private_root_invalid"));
    }
    Ok(canonical)
}

fn create_or_verify_private_child(parent: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.chars().any(char::is_control)
    {
        return Err(checkpoint_error("checkpoint_fixed_child_name_invalid"));
    }
    let child = parent.join(name);
    reject_symlink(&child, "checkpoint_fixed_child_is_symlink")?;
    match fs::create_dir(&child) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(checkpoint_error("checkpoint_fixed_child_create_failed")),
    }
    reject_symlink(&child, "checkpoint_fixed_child_is_symlink")?;
    let canonical = fs::canonicalize(&child)
        .map_err(|_| checkpoint_error("checkpoint_fixed_child_unavailable"))?;
    if canonical != child || canonical.parent() != Some(parent) {
        return Err(checkpoint_error(
            "checkpoint_fixed_child_containment_failed",
        ));
    }
    set_private_dir_mode(&canonical)?;
    Ok(canonical)
}

fn reject_symlink(path: &Path, code: &'static str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(checkpoint_error(code)),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(checkpoint_error("checkpoint_path_metadata_failed")),
    }
}

fn set_private_dir_mode(path: &Path) -> Result<()> {
    let mut permissions = fs::metadata(path)
        .map_err(|_| checkpoint_error("checkpoint_directory_mode_stat_failed"))?
        .permissions();
    permissions.set_mode(PRIVATE_DIR_MODE);
    fs::set_permissions(path, permissions)
        .map_err(|_| checkpoint_error("checkpoint_directory_mode_failed"))
}

fn sync_dir(path: &Path) -> Result<()> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let dir = options
        .open(path)
        .map_err(|_| checkpoint_error("checkpoint_directory_open_failed"))?;
    dir.sync_all()
        .map_err(|_| checkpoint_error("checkpoint_directory_sync_failed"))
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn exchange_children(parent: &Path, left: &str, right: &str) -> Result<()> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let parent_fd = options
        .open(parent)
        .map_err(|_| checkpoint_error("checkpoint_exchange_parent_open_failed"))?;
    let left =
        CString::new(left).map_err(|_| checkpoint_error("checkpoint_exchange_name_invalid"))?;
    let right =
        CString::new(right).map_err(|_| checkpoint_error("checkpoint_exchange_name_invalid"))?;
    let result = unsafe {
        libc::renameat2(
            parent_fd.as_raw_fd(),
            left.as_ptr(),
            parent_fd.as_raw_fd(),
            right.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    if result != 0 {
        return Err(checkpoint_error("checkpoint_atomic_exchange_failed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("vibecoder-part17-{label}-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        root
    }

    #[test]
    fn snapshot_digest_detects_changes_and_rejects_links() {
        let root = temp_root("digest");
        let source = root.join("source");
        let copy = root.join("copy");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&copy).unwrap();
        fs::write(source.join("a.txt"), b"one").unwrap();
        let first = copy_tree_and_digest(&source, &copy).unwrap();
        assert_eq!(first, digest_tree(&copy).unwrap());
        fs::write(source.join("a.txt"), b"two").unwrap();
        assert_ne!(first, digest_tree(&source).unwrap());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    #[test]
    fn published_checkpoint_restores_complete_project_tree() {
        let root = temp_root("rollback");
        let store = LocalCheckpointStore::initialize(LocalCheckpointConfig {
            app_private_dir: root.clone(),
        })
        .unwrap();
        let project_id = ProjectId::new();
        let project_root = store
            .projects_root
            .join(project_id.0.hyphenated().to_string());
        fs::create_dir(&project_root).unwrap();
        set_private_dir_mode(&project_root).unwrap();
        fs::create_dir(project_root.join("src")).unwrap();
        fs::write(project_root.join("src/main.txt"), b"before").unwrap();
        let project = ProjectRef {
            id: project_id,
            root: project_root.clone(),
        };

        let checkpoint = store
            .create_checkpoint_sync(&project, CheckpointReason::BeforeAgentChange)
            .unwrap();
        fs::write(project_root.join("src/main.txt"), b"broken").unwrap();
        fs::write(project_root.join("new.txt"), b"should disappear").unwrap();

        let result = store
            .rollback_sync(&project, checkpoint.checkpoint_id)
            .unwrap();
        assert_eq!(result.tree_sha256, checkpoint.tree_sha256);
        assert_eq!(
            fs::read(project_root.join("src/main.txt")).unwrap(),
            b"before"
        );
        assert!(!project_root.join("new.txt").exists());
        assert!(
            store
                .checkpoint_dir(project_id, checkpoint.checkpoint_id)
                .is_dir()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn hard_links_are_not_snapshot_material() {
        let root = temp_root("hardlink");
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("a.txt"), b"one").unwrap();
        fs::hard_link(source.join("a.txt"), source.join("b.txt")).unwrap();
        assert!(digest_tree(&source).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
