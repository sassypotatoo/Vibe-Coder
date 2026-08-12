//! App-private local project-state store for Android/Unix.
//!
//! State is stored outside agent-visible project roots under fixed VibeCoder-owned directories.
//! Every file operation re-enters from the canonical app-private root and opens fixed descendants
//! with `O_NOFOLLOW`; persisted project ids are used only to derive fixed UUID filenames.

use async_trait::async_trait;
use std::fs;
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use vibecoder_domain::{ProjectId, Result, VibeCoderError};
use vibecoder_persistence_contract::{
    MAX_PERSISTED_PROJECTS, PersistedProjectState, PersistenceCapabilities, ProjectStateStore,
};

#[cfg(unix)]
mod unix_store;

const PRODUCT_ROOT_NAME: &str = "vibecoder";
const STATE_ROOT_NAME: &str = "state";
const PROJECT_STATE_ROOT_NAME: &str = "projects";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalProjectStateConfig {
    /// Existing application-private directory supplied by Android/platform code.
    pub app_private_dir: PathBuf,
}

#[derive(Debug)]
pub struct LocalProjectStateStore {
    app_private_root: PathBuf,
    project_state_root: PathBuf,
    gate: Mutex<()>,
}

impl LocalProjectStateStore {
    pub fn initialize(config: LocalProjectStateConfig) -> Result<Self> {
        if !config.app_private_dir.is_absolute() {
            return Err(persistence_error("app_private_root_not_absolute"));
        }
        reject_symlink_if_present(&config.app_private_dir, "app_private_root_is_symlink")?;
        let app_private_root =
            canonical_existing_dir(&config.app_private_dir, "app_private_root_unavailable")?;
        let product_root = create_or_verify_fixed_child(
            &app_private_root,
            PRODUCT_ROOT_NAME,
            "product_root_invalid",
        )?;
        let state_root =
            create_or_verify_fixed_child(&product_root, STATE_ROOT_NAME, "state_root_invalid")?;
        let project_state_root = create_or_verify_fixed_child(
            &state_root,
            PROJECT_STATE_ROOT_NAME,
            "project_state_root_invalid",
        )?;
        if !project_state_root.starts_with(&app_private_root) {
            return Err(persistence_error(
                "project_state_root_escaped_app_private_root",
            ));
        }
        Ok(Self {
            app_private_root,
            project_state_root,
            gate: Mutex::new(()),
        })
    }

    pub fn project_state_root(&self) -> &Path {
        &self.project_state_root
    }

    fn lock_gate(&self) -> Result<MutexGuard<'_, ()>> {
        self.gate
            .lock()
            .map_err(|_| persistence_error("project_state_gate_poisoned"))
    }
}

#[async_trait]
impl ProjectStateStore for LocalProjectStateStore {
    fn capabilities(&self) -> PersistenceCapabilities {
        PersistenceCapabilities {
            project_registry: true,
            session_binding: true,
            model_preference: true,
            route_policy: true,
            atomic_replace: cfg!(unix),
            secrets_persisted: false,
        }
    }

    async fn create_project_state(&self, state: &PersistedProjectState) -> Result<()> {
        state.validate()?;
        if state.revision != 0 {
            return Err(persistence_error("project_state_create_revision_invalid"));
        }
        let _gate = self.lock_gate()?;
        #[cfg(unix)]
        {
            if unix_store::load_project_state(self, state.project_id)?.is_some() {
                return Err(persistence_error("project_state_already_exists"));
            }
            return unix_store::save_project_state(self, state);
        }
        #[cfg(not(unix))]
        {
            let _ = state;
            Err(persistence_error("secure_persistence_unsupported_platform"))
        }
    }

    async fn update_project_state(
        &self,
        expected_revision: u64,
        state: &PersistedProjectState,
    ) -> Result<PersistedProjectState> {
        state.validate()?;
        if state.revision != expected_revision {
            return Err(persistence_error(
                "project_state_expected_revision_mismatch",
            ));
        }
        let _gate = self.lock_gate()?;
        #[cfg(unix)]
        {
            let current = unix_store::load_project_state(self, state.project_id)?
                .ok_or_else(|| persistence_error("project_state_not_found"))?;
            if current.revision != expected_revision {
                return Err(persistence_error("project_state_revision_conflict"));
            }
            let mut committed = state.clone();
            committed.revision = expected_revision
                .checked_add(1)
                .ok_or_else(|| persistence_error("project_state_revision_overflow"))?;
            unix_store::save_project_state(self, &committed)?;
            return Ok(committed);
        }
        #[cfg(not(unix))]
        {
            let _ = (expected_revision, state);
            Err(persistence_error("secure_persistence_unsupported_platform"))
        }
    }

    async fn load_project_state(
        &self,
        project_id: ProjectId,
    ) -> Result<Option<PersistedProjectState>> {
        let _gate = self.lock_gate()?;
        #[cfg(unix)]
        {
            return unix_store::load_project_state(self, project_id);
        }
        #[cfg(not(unix))]
        {
            let _ = project_id;
            Err(persistence_error("secure_persistence_unsupported_platform"))
        }
    }

    async fn list_project_ids(&self, max_projects: usize) -> Result<Vec<ProjectId>> {
        if max_projects == 0 || max_projects > MAX_PERSISTED_PROJECTS {
            return Err(persistence_error("project_state_list_limit_invalid"));
        }
        let _gate = self.lock_gate()?;
        #[cfg(any(target_os = "android", target_os = "linux"))]
        {
            return unix_store::list_project_ids(self, max_projects);
        }
        #[cfg(not(any(target_os = "android", target_os = "linux")))]
        {
            let _ = max_projects;
            Err(persistence_error(
                "secure_persistence_listing_unsupported_platform",
            ))
        }
    }

    async fn remove_project_state(&self, project_id: ProjectId) -> Result<()> {
        let _gate = self.lock_gate()?;
        #[cfg(unix)]
        {
            return unix_store::remove_project_state(self, project_id);
        }
        #[cfg(not(unix))]
        {
            let _ = project_id;
            Err(persistence_error("secure_persistence_unsupported_platform"))
        }
    }
}

fn canonical_existing_dir(path: &Path, code: &'static str) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path).map_err(|_| persistence_error(code))?;
    let metadata = fs::metadata(&canonical).map_err(|_| persistence_error(code))?;
    if !metadata.is_dir() {
        return Err(persistence_error(code));
    }
    Ok(canonical)
}

fn reject_symlink_if_present(path: &Path, code: &'static str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(persistence_error(code)),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(_) => Err(persistence_error("state_path_metadata_failed")),
    }
}

fn create_or_verify_fixed_child(parent: &Path, name: &str, code: &'static str) -> Result<PathBuf> {
    let child = parent.join(name);
    reject_symlink_if_present(&child, code)?;
    match fs::create_dir(&child) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(_) => return Err(persistence_error(code)),
    }
    reject_symlink_if_present(&child, code)?;
    let canonical = canonical_existing_dir(&child, code)?;
    if canonical != child || !canonical.starts_with(parent) {
        return Err(persistence_error(code));
    }
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&canonical)
            .map_err(|_| persistence_error(code))?
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&canonical, permissions).map_err(|_| persistence_error(code))?;
    }
    Ok(canonical)
}

fn persistence_error(code: &'static str) -> VibeCoderError {
    VibeCoderError::Persistence(code.into())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use uuid::Uuid;
    use vibecoder_persistence_contract::PersistedProjectState;

    fn temp_app_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "vibecoder-part16-{label}-{}",
            Uuid::new_v4().simple()
        ));
        fs::create_dir(&root).unwrap();
        root
    }

    #[test]
    fn state_round_trip_and_aliases_fail_closed() {
        let app_root = temp_app_root("state");
        let store = LocalProjectStateStore::initialize(LocalProjectStateConfig {
            app_private_dir: app_root.clone(),
        })
        .unwrap();
        let project_id = ProjectId(Uuid::new_v4());
        let state = PersistedProjectState::new(project_id);
        unix_store::save_project_state(&store, &state).unwrap();
        assert_eq!(
            unix_store::load_project_state(&store, project_id).unwrap(),
            Some(state.clone())
        );

        let state_path = store
            .project_state_root()
            .join(format!("{}.json", project_id.0.hyphenated()));
        let hard_link = app_root.join("state-hard-link-alias");
        fs::hard_link(&state_path, &hard_link).unwrap();
        assert!(unix_store::load_project_state(&store, project_id).is_err());
        fs::remove_file(&hard_link).unwrap();

        fs::remove_file(&state_path).unwrap();
        let outside = app_root.join("outside-state.json");
        fs::write(&outside, b"{}").unwrap();
        symlink(&outside, &state_path).unwrap();
        assert!(unix_store::load_project_state(&store, project_id).is_err());

        let _ = fs::remove_dir_all(app_root);
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    #[test]
    fn listing_rejects_noncanonical_uuid_spelling() {
        let app_root = temp_app_root("listing");
        let store = LocalProjectStateStore::initialize(LocalProjectStateConfig {
            app_private_dir: app_root.clone(),
        })
        .unwrap();
        let project_id = ProjectId(Uuid::new_v4());
        let state = PersistedProjectState::new(project_id);
        unix_store::save_project_state(&store, &state).unwrap();

        let canonical = store
            .project_state_root()
            .join(format!("{}.json", project_id.0.hyphenated()));
        let upper = store.project_state_root().join(format!(
            "{}.json",
            project_id.0.hyphenated().to_string().to_uppercase()
        ));
        fs::copy(&canonical, &upper).unwrap();
        let mut permissions = fs::metadata(&upper).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o600);
        fs::set_permissions(&upper, permissions).unwrap();

        assert!(unix_store::list_project_ids(&store, 16).is_err());
        let _ = fs::remove_dir_all(app_root);
    }
}
