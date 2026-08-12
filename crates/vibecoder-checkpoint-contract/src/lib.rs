//! Provider-neutral checkpoint/snapshot metadata and rollback contract.
//!
//! A checkpoint is a private immutable copy of one managed project tree plus bounded integrity
//! metadata. Checkpoint ids and digests are evidence, not filesystem authority; local adapters must
//! derive all physical paths beneath their app-private roots and re-verify them at operation time.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vibecoder_domain::{ProjectId, ProjectRef, Result, VibeCoderError};

pub const CHECKPOINT_SCHEMA_V1: u32 = 1;
pub const MAX_CHECKPOINTS_PER_PROJECT: usize = 64;
pub const MAX_CHECKPOINT_FILES: u64 = 100_000;
pub const MAX_CHECKPOINT_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_CHECKPOINT_DEPTH: usize = 128;
pub const SHA256_HEX_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CheckpointId(pub Uuid);

impl CheckpointId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for CheckpointId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointReason {
    Manual,
    BeforeAgentChange,
    BeforeBuildRepair,
    BeforeRollback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointMetadata {
    pub schema: u32,
    pub checkpoint_id: CheckpointId,
    pub project_id: ProjectId,
    pub created_unix_ms: u64,
    pub reason: CheckpointReason,
    pub file_count: u64,
    pub total_bytes: u64,
    pub tree_sha256: String,
}

impl CheckpointMetadata {
    pub fn validate(&self) -> Result<()> {
        if self.schema != CHECKPOINT_SCHEMA_V1 {
            return Err(checkpoint_error("checkpoint_schema_unsupported"));
        }
        if self.file_count > MAX_CHECKPOINT_FILES {
            return Err(checkpoint_error("checkpoint_file_count_invalid"));
        }
        if self.total_bytes > MAX_CHECKPOINT_TOTAL_BYTES {
            return Err(checkpoint_error("checkpoint_total_bytes_invalid"));
        }
        if self.tree_sha256.len() != SHA256_HEX_BYTES
            || !self
                .tree_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        {
            return Err(checkpoint_error("checkpoint_digest_invalid"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointCapabilities {
    pub immutable_snapshots: bool,
    pub integrity_digest: bool,
    pub rollback: bool,
    pub atomic_project_exchange: bool,
    pub secrets_indexed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackResult {
    pub project_id: ProjectId,
    pub checkpoint_id: CheckpointId,
    pub restored_file_count: u64,
    pub restored_total_bytes: u64,
    pub tree_sha256: String,
}

#[async_trait]
pub trait CheckpointStore: Send + Sync {
    fn capabilities(&self) -> CheckpointCapabilities;

    async fn create_checkpoint(
        &self,
        project: &ProjectRef,
        reason: CheckpointReason,
    ) -> Result<CheckpointMetadata>;

    async fn list_checkpoints(
        &self,
        project_id: ProjectId,
        max_results: usize,
    ) -> Result<Vec<CheckpointMetadata>>;

    async fn load_checkpoint(
        &self,
        project_id: ProjectId,
        checkpoint_id: CheckpointId,
    ) -> Result<Option<CheckpointMetadata>>;

    async fn rollback_project(
        &self,
        project: &ProjectRef,
        checkpoint_id: CheckpointId,
    ) -> Result<RollbackResult>;

    async fn remove_checkpoint(
        &self,
        project_id: ProjectId,
        checkpoint_id: CheckpointId,
    ) -> Result<()>;
}

pub fn checkpoint_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Checkpoint(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_rejects_non_lower_hex_digest() {
        let metadata = CheckpointMetadata {
            schema: CHECKPOINT_SCHEMA_V1,
            checkpoint_id: CheckpointId(Uuid::nil()),
            project_id: ProjectId(Uuid::nil()),
            created_unix_ms: 1,
            reason: CheckpointReason::Manual,
            file_count: 0,
            total_bytes: 0,
            tree_sha256: "A".repeat(SHA256_HEX_BYTES),
        };
        assert!(metadata.validate().is_err());
    }
}
