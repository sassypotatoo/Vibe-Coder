use jcode_sdk::SessionInfo;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use vibecoder_domain::{ProjectId, ProjectRef, Result, SessionId, VibeCoderError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionBinding {
    pub project_id: ProjectId,
    pub project_root: PathBuf,
    pub connection_generation: u64,
}

#[derive(Debug, Default)]
struct SessionRegistryState {
    bindings: HashMap<String, SessionBinding>,
    attached_session: Option<String>,
}

/// In-memory session/project bindings for the current application process.
///
/// Part 16 persists only stable project/session identity in the separate app-private state store.
/// This registry deliberately remains memory-only because canonical roots, attachment state, and
/// connection generations are live authorization facts that must be rebuilt and corroborated after
/// restart; its job is to prevent a session id from silently drifting to a different project while
/// the Jcode connection switches attachments or reconnects.
pub(crate) struct SessionRegistry {
    state: RwLock<SessionRegistryState>,
    gate: Mutex<()>,
}

impl SessionRegistry {
    pub(crate) fn new() -> Self {
        Self {
            state: RwLock::new(SessionRegistryState::default()),
            gate: Mutex::new(()),
        }
    }

    pub(crate) fn lock_gate(&self) -> Result<MutexGuard<'_, ()>> {
        self.gate
            .lock()
            .map_err(|_| VibeCoderError::Agent("Jcode session gate poisoned".into()))
    }

    pub(crate) fn binding(&self, session_id: &SessionId) -> Result<Option<SessionBinding>> {
        Ok(self.read()?.bindings.get(&session_id.0).cloned())
    }

    pub(crate) fn is_attached_on_generation(
        &self,
        session_id: &SessionId,
        generation: u64,
    ) -> Result<bool> {
        let state = self.read()?;
        let active = state.attached_session.as_deref() == Some(session_id.0.as_str());
        let current = state
            .bindings
            .get(&session_id.0)
            .is_some_and(|binding| binding.connection_generation == generation);
        Ok(active && current)
    }

    pub(crate) fn mark_attached(
        &self,
        session_id: &SessionId,
        project: &ProjectRef,
        project_root: PathBuf,
        generation: u64,
    ) -> Result<()> {
        let mut state = self.write()?;
        if let Some(existing) = state.bindings.get(&session_id.0)
            && (existing.project_id != project.id || existing.project_root != project_root)
        {
            return Err(VibeCoderError::InvalidRequest(
                "session id is already bound to a different project".into(),
            ));
        }
        state.bindings.insert(
            session_id.0.clone(),
            SessionBinding {
                project_id: project.id,
                project_root,
                connection_generation: generation,
            },
        );
        state.attached_session = Some(session_id.0.clone());
        Ok(())
    }

    pub(crate) fn clear_attachment(&self) -> Result<()> {
        self.write()?.attached_session = None;
        Ok(())
    }

    fn read(&self) -> Result<RwLockReadGuard<'_, SessionRegistryState>> {
        self.state
            .read()
            .map_err(|_| VibeCoderError::Agent("Jcode session registry lock poisoned".into()))
    }

    fn write(&self) -> Result<RwLockWriteGuard<'_, SessionRegistryState>> {
        self.state
            .write()
            .map_err(|_| VibeCoderError::Agent("Jcode session registry lock poisoned".into()))
    }
}

pub(crate) fn canonical_project_root(project: &ProjectRef) -> Result<PathBuf> {
    if !project.root.is_absolute() {
        return Err(VibeCoderError::InvalidRequest(
            "project root must be an absolute path before creating an agent session".into(),
        ));
    }

    let canonical = fs::canonicalize(&project.root).map_err(|_| {
        VibeCoderError::InvalidRequest(
            "project root must exist and be accessible before creating an agent session".into(),
        )
    })?;
    if !canonical.is_dir() {
        return Err(VibeCoderError::InvalidRequest(
            "project root must point to a directory".into(),
        ));
    }
    if canonical.to_str().is_none() {
        return Err(VibeCoderError::InvalidRequest(
            "project root must be valid UTF-8 for the Jcode harness".into(),
        ));
    }
    Ok(canonical)
}

/// The create/attach reply identifies the session, but the reviewed Jcode 0.73.0 bridge currently
/// leaves `working_dir` empty on that reply. Therefore project verification must use the persisted
/// metadata returned by `list_sessions`, not the immediate attach event.
pub(crate) fn validate_jcode_session_id(session_id: &SessionId) -> Result<()> {
    let value = session_id.0.as_str();
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(VibeCoderError::InvalidRequest(
            "Jcode session id has an invalid format".into(),
        ));
    }
    Ok(())
}

pub(crate) fn verify_attached_session_id(
    attached: &SessionInfo,
    expected_session: Option<&SessionId>,
) -> Result<SessionId> {
    let actual_id = SessionId::parse(attached.session_id.clone()).map_err(|_| {
        VibeCoderError::Agent("Jcode returned an invalid session identifier".into())
    })?;
    validate_jcode_session_id(&actual_id).map_err(|_| {
        VibeCoderError::Agent("Jcode returned a malformed session identifier".into())
    })?;
    if let Some(expected) = expected_session
        && actual_id != *expected
    {
        return Err(VibeCoderError::Agent(
            "Jcode attached a different session than requested".into(),
        ));
    }
    Ok(actual_id)
}

pub(crate) fn session_metadata<'a>(
    sessions: &'a [SessionInfo],
    session_id: &SessionId,
) -> Result<&'a SessionInfo> {
    let mut matches = sessions
        .iter()
        .filter(|candidate| candidate.session_id == session_id.0);
    let first = matches.next().ok_or_else(|| {
        VibeCoderError::Agent(
            "Jcode session metadata is unavailable, so project identity cannot be verified".into(),
        )
    })?;
    if matches.next().is_some() {
        return Err(VibeCoderError::Agent(
            "Jcode returned duplicate metadata for one session".into(),
        ));
    }
    Ok(first)
}

pub(crate) fn corroborate_new_session_project(
    sessions: &[SessionInfo],
    session_id: &SessionId,
    expected_root: &Path,
) -> Result<()> {
    let matches: Vec<&SessionInfo> = sessions
        .iter()
        .filter(|candidate| candidate.session_id == session_id.0)
        .collect();
    if matches.len() > 1 {
        return Err(VibeCoderError::Agent(
            "Jcode returned duplicate metadata for one session".into(),
        ));
    }
    let Some(metadata) = matches.first().copied() else {
        // A newly-created session record can still be mid-write. Creation is rooted by the exact
        // canonical working_dir we supplied to Jcode; persisted metadata is corroboration here,
        // not the sole source of authorization. Resume remains strict.
        return Ok(());
    };
    if metadata.working_dir.is_none() {
        return Ok(());
    }
    verify_session_project(metadata, expected_root)
}

pub(crate) fn verify_session_project(metadata: &SessionInfo, expected_root: &Path) -> Result<()> {
    let working_dir = metadata.working_dir.as_deref().ok_or_else(|| {
        VibeCoderError::Agent("Jcode session did not report a working directory".into())
    })?;
    if working_dir.trim().is_empty() {
        return Err(VibeCoderError::Agent(
            "Jcode session reported an empty working directory".into(),
        ));
    }

    let path = Path::new(working_dir);
    if !path.is_absolute() {
        return Err(VibeCoderError::Agent(
            "Jcode session reported a relative working directory; project identity is ambiguous"
                .into(),
        ));
    }
    let actual_root = fs::canonicalize(path).map_err(|_| {
        VibeCoderError::Agent("Jcode session working directory could not be verified".into())
    })?;
    if actual_root != expected_root {
        return Err(VibeCoderError::Agent(
            "Jcode session working directory does not match the expected project".into(),
        ));
    }
    Ok(())
}
