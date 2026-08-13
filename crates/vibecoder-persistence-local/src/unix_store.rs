use super::{
    CONVERSATION_STATE_ROOT_NAME, LocalProjectStateStore, PRODUCT_ROOT_NAME,
    PROJECT_STATE_ROOT_NAME, STATE_ROOT_NAME, persistence_error,
};
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use uuid::Uuid;
use vibecoder_domain::{ConversationId, ProjectId, Result};
use vibecoder_persistence_contract::{
    MAX_PERSISTED_CONVERSATION_BYTES, MAX_PERSISTED_CONVERSATIONS_PER_PROJECT,
    MAX_PERSISTED_STATE_BYTES, PersistedConversation, PersistedProjectState,
};

const PRIVATE_FILE_MODE: libc::mode_t = 0o600;
const MAX_STATE_DIRECTORY_ENTRIES: usize = 8192;
const TEMP_PREFIX: &str = ".vibecoder-state-tmp-";
const CONVERSATION_TEMP_PREFIX: &str = ".vibecoder-conversation-tmp-";

pub(super) fn save_project_state(
    store: &LocalProjectStateStore,
    state: &PersistedProjectState,
) -> Result<()> {
    let encoded = serde_json::to_vec(state)
        .map_err(|_| persistence_error("project_state_serialize_failed"))?;
    if encoded.len() > MAX_PERSISTED_STATE_BYTES {
        return Err(persistence_error("project_state_too_large"));
    }

    let root = open_project_state_root(store)?;
    let target = state_file_name(state.project_id)?;
    inspect_existing_state_target(root.as_raw_fd(), &target)?;

    let temp = CString::new(format!("{TEMP_PREFIX}{}", Uuid::new_v4().simple()))
        .map_err(|_| persistence_error("project_state_temp_name_invalid"))?;
    let temp_fd = open_exclusive_private_file_at(root.as_raw_fd(), &temp)?;
    let mut cleanup = TempCleanup {
        parent_fd: root.as_raw_fd(),
        name: &temp,
        armed: true,
    };
    verify_private_regular_fd(temp_fd.as_raw_fd(), "project_state_temp_invalid")?;

    let mut file: File = temp_fd.into();
    file.write_all(&encoded)
        .map_err(|_| persistence_error("project_state_write_failed"))?;
    file.sync_all()
        .map_err(|_| persistence_error("project_state_file_sync_failed"))?;
    drop(file);

    if unsafe {
        libc::renameat(
            root.as_raw_fd(),
            temp.as_ptr(),
            root.as_raw_fd(),
            target.as_ptr(),
        )
    } != 0
    {
        return Err(persistence_error("project_state_rename_failed"));
    }
    cleanup.armed = false;

    if unsafe { libc::fsync(root.as_raw_fd()) } != 0 {
        return Err(persistence_error("project_state_parent_sync_failed"));
    }
    Ok(())
}

pub(super) fn load_project_state(
    store: &LocalProjectStateStore,
    project_id: ProjectId,
) -> Result<Option<PersistedProjectState>> {
    let root = open_project_state_root(store)?;
    let name = state_file_name(project_id)?;
    let Some(expected) = inspect_state_for_read(root.as_raw_fd(), &name)? else {
        return Ok(None);
    };
    if expected.st_size < 0 || expected.st_size as u64 > MAX_PERSISTED_STATE_BYTES as u64 {
        return Err(persistence_error("project_state_too_large"));
    }

    let fd = open_file_at(root.as_raw_fd(), &name, libc::O_RDONLY)?;
    let actual = fstat(fd.as_raw_fd(), "project_state_stat_failed")?;
    verify_private_regular_stat(&actual, "project_state_target_invalid")?;
    if actual.st_dev != expected.st_dev || actual.st_ino != expected.st_ino {
        return Err(persistence_error("project_state_changed_during_open"));
    }

    let mut file: File = fd.into();
    let mut bytes = Vec::with_capacity((actual.st_size as usize).min(MAX_PERSISTED_STATE_BYTES));
    let mut bounded = (&mut file).take(MAX_PERSISTED_STATE_BYTES as u64 + 1);
    bounded
        .read_to_end(&mut bytes)
        .map_err(|_| persistence_error("project_state_read_failed"))?;
    if bytes.len() > MAX_PERSISTED_STATE_BYTES {
        return Err(persistence_error("project_state_too_large"));
    }

    let state: PersistedProjectState = serde_json::from_slice(&bytes)
        .map_err(|_| persistence_error("project_state_invalid_json"))?;
    state.validate()?;
    if state.project_id != project_id {
        return Err(persistence_error("project_state_id_mismatch"));
    }
    Ok(Some(state))
}

#[cfg(any(target_os = "android", target_os = "linux"))]
pub(super) fn list_project_ids(
    store: &LocalProjectStateStore,
    max_projects: usize,
) -> Result<Vec<ProjectId>> {
    let root = open_project_state_root(store)?;
    let duplicate = unsafe { libc::dup(root.as_raw_fd()) };
    if duplicate < 0 {
        return Err(persistence_error("project_state_list_dup_failed"));
    }
    let dir = unsafe { libc::fdopendir(duplicate) };
    if dir.is_null() {
        unsafe { libc::close(duplicate) };
        return Err(persistence_error("project_state_list_open_failed"));
    }
    let guard = DirGuard(dir);
    let mut ids = Vec::new();
    let mut entries_seen = 0usize;

    loop {
        set_errno_zero();
        let entry = unsafe { libc::readdir(guard.0) };
        if entry.is_null() {
            if current_errno() != 0 {
                return Err(persistence_error("project_state_list_read_failed"));
            }
            break;
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        let bytes = name.to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        entries_seen = entries_seen
            .checked_add(1)
            .ok_or_else(|| persistence_error("project_state_directory_too_large"))?;
        if entries_seen > MAX_STATE_DIRECTORY_ENTRIES {
            return Err(persistence_error("project_state_directory_too_large"));
        }
        if bytes.starts_with(TEMP_PREFIX.as_bytes()) {
            continue;
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| persistence_error("project_state_directory_unexpected_entry"))?;
        let stem = text
            .strip_suffix(".json")
            .ok_or_else(|| persistence_error("project_state_directory_unexpected_entry"))?;
        let uuid = Uuid::parse_str(stem)
            .map_err(|_| persistence_error("project_state_directory_unexpected_entry"))?;
        if stem != uuid.hyphenated().to_string() {
            return Err(persistence_error(
                "project_state_directory_unexpected_entry",
            ));
        }
        let name_c = CString::new(bytes)
            .map_err(|_| persistence_error("project_state_directory_unexpected_entry"))?;
        let stat = fstatat_nofollow(root.as_raw_fd(), &name_c, "project_state_list_stat_failed")?;
        verify_private_regular_stat(&stat, "project_state_target_invalid")?;
        ids.push(ProjectId(uuid));
        if ids.len() > max_projects {
            return Err(persistence_error("project_state_list_limit_exceeded"));
        }
    }
    drop(guard);
    ids.sort_by_key(|id| id.0.hyphenated().to_string());
    Ok(ids)
}



pub(super) fn save_conversation(
    store: &LocalProjectStateStore,
    conversation: &PersistedConversation,
) -> Result<()> {
    let encoded = serde_json::to_vec(conversation)
        .map_err(|_| persistence_error("conversation_serialize_failed"))?;
    if encoded.len() > MAX_PERSISTED_CONVERSATION_BYTES {
        return Err(persistence_error("conversation_too_large"));
    }

    let root = open_conversation_state_root(store)?;
    let target = conversation_file_name(conversation.project_id, conversation.conversation_id)?;
    inspect_existing_state_target(root.as_raw_fd(), &target)?;

    let temp = CString::new(format!(
        "{CONVERSATION_TEMP_PREFIX}{}",
        Uuid::new_v4().simple()
    ))
    .map_err(|_| persistence_error("conversation_temp_name_invalid"))?;
    let temp_fd = open_exclusive_private_file_at(root.as_raw_fd(), &temp)?;
    let mut cleanup = TempCleanup {
        parent_fd: root.as_raw_fd(),
        name: &temp,
        armed: true,
    };
    verify_private_regular_fd(temp_fd.as_raw_fd(), "conversation_temp_invalid")?;

    let mut file: File = temp_fd.into();
    file.write_all(&encoded)
        .map_err(|_| persistence_error("conversation_write_failed"))?;
    file.sync_all()
        .map_err(|_| persistence_error("conversation_file_sync_failed"))?;
    drop(file);

    if unsafe {
        libc::renameat(
            root.as_raw_fd(),
            temp.as_ptr(),
            root.as_raw_fd(),
            target.as_ptr(),
        )
    } != 0
    {
        return Err(persistence_error("conversation_rename_failed"));
    }
    cleanup.armed = false;

    if unsafe { libc::fsync(root.as_raw_fd()) } != 0 {
        return Err(persistence_error("conversation_parent_sync_failed"));
    }
    Ok(())
}

pub(super) fn load_conversation(
    store: &LocalProjectStateStore,
    project_id: ProjectId,
    conversation_id: ConversationId,
) -> Result<Option<PersistedConversation>> {
    let root = open_conversation_state_root(store)?;
    let name = conversation_file_name(project_id, conversation_id)?;
    let Some(expected) = inspect_state_for_read(root.as_raw_fd(), &name)? else {
        return Ok(None);
    };
    if expected.st_size < 0 || expected.st_size as u64 > MAX_PERSISTED_CONVERSATION_BYTES as u64 {
        return Err(persistence_error("conversation_too_large"));
    }

    let fd = open_file_at(root.as_raw_fd(), &name, libc::O_RDONLY)?;
    let actual = fstat(fd.as_raw_fd(), "conversation_stat_failed")?;
    verify_private_regular_stat(&actual, "conversation_target_invalid")?;
    if actual.st_dev != expected.st_dev || actual.st_ino != expected.st_ino {
        return Err(persistence_error("conversation_changed_during_open"));
    }

    let mut file: File = fd.into();
    let mut bytes = Vec::with_capacity(
        (actual.st_size as usize).min(MAX_PERSISTED_CONVERSATION_BYTES),
    );
    let mut bounded = (&mut file).take(MAX_PERSISTED_CONVERSATION_BYTES as u64 + 1);
    bounded
        .read_to_end(&mut bytes)
        .map_err(|_| persistence_error("conversation_read_failed"))?;
    if bytes.len() > MAX_PERSISTED_CONVERSATION_BYTES {
        return Err(persistence_error("conversation_too_large"));
    }

    let conversation: PersistedConversation = serde_json::from_slice(&bytes)
        .map_err(|_| persistence_error("conversation_invalid_json"))?;
    conversation.validate()?;
    if conversation.project_id != project_id || conversation.conversation_id != conversation_id {
        return Err(persistence_error("conversation_id_mismatch"));
    }
    Ok(Some(conversation))
}

#[cfg(any(target_os = "android", target_os = "linux"))]
pub(super) fn list_conversation_ids(
    store: &LocalProjectStateStore,
    project_id: ProjectId,
    max_conversations: usize,
) -> Result<Vec<ConversationId>> {
    let root = open_conversation_state_root(store)?;
    let duplicate = unsafe { libc::dup(root.as_raw_fd()) };
    if duplicate < 0 {
        return Err(persistence_error("conversation_list_dup_failed"));
    }
    let dir = unsafe { libc::fdopendir(duplicate) };
    if dir.is_null() {
        unsafe { libc::close(duplicate) };
        return Err(persistence_error("conversation_list_open_failed"));
    }
    let guard = DirGuard(dir);
    let mut ids = Vec::new();
    let mut entries_seen = 0usize;

    loop {
        set_errno_zero();
        let entry = unsafe { libc::readdir(guard.0) };
        if entry.is_null() {
            if current_errno() != 0 {
                return Err(persistence_error("conversation_list_read_failed"));
            }
            break;
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        let bytes = name.to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        entries_seen = entries_seen
            .checked_add(1)
            .ok_or_else(|| persistence_error("conversation_directory_too_large"))?;
        if entries_seen > MAX_STATE_DIRECTORY_ENTRIES {
            return Err(persistence_error("conversation_directory_too_large"));
        }
        if bytes.starts_with(CONVERSATION_TEMP_PREFIX.as_bytes()) {
            continue;
        }

        let text = std::str::from_utf8(bytes)
            .map_err(|_| persistence_error("conversation_directory_unexpected_entry"))?;
        let (entry_project_id, entry_conversation_id) = parse_conversation_file_name(text)?;
        let name_c = CString::new(bytes)
            .map_err(|_| persistence_error("conversation_directory_unexpected_entry"))?;
        let stat = fstatat_nofollow(root.as_raw_fd(), &name_c, "conversation_list_stat_failed")?;
        verify_private_regular_stat(&stat, "conversation_target_invalid")?;
        if entry_project_id == project_id {
            ids.push(entry_conversation_id);
            if ids.len() > max_conversations {
                return Err(persistence_error("conversation_list_limit_exceeded"));
            }
        }
    }
    drop(guard);
    ids.sort_by_key(|id| id.0.hyphenated().to_string());
    Ok(ids)
}

pub(super) fn remove_conversation(
    store: &LocalProjectStateStore,
    project_id: ProjectId,
    conversation_id: ConversationId,
) -> Result<()> {
    let root = open_conversation_state_root(store)?;
    let name = conversation_file_name(project_id, conversation_id)?;
    if inspect_state_for_read(root.as_raw_fd(), &name)?.is_none() {
        return Ok(());
    }
    if unsafe { libc::unlinkat(root.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != ErrorKind::NotFound {
            return Err(persistence_error("conversation_remove_failed"));
        }
    }
    if unsafe { libc::fsync(root.as_raw_fd()) } != 0 {
        return Err(persistence_error("conversation_parent_sync_failed"));
    }
    Ok(())
}

#[cfg(any(target_os = "android", target_os = "linux"))]
pub(super) fn remove_project_conversations(
    store: &LocalProjectStateStore,
    project_id: ProjectId,
) -> Result<()> {
    let ids = list_conversation_ids(store, project_id, MAX_PERSISTED_CONVERSATIONS_PER_PROJECT)?;
    for conversation_id in ids {
        remove_conversation(store, project_id, conversation_id)?;
    }
    Ok(())
}

pub(super) fn remove_project_state(
    store: &LocalProjectStateStore,
    project_id: ProjectId,
) -> Result<()> {
    let root = open_project_state_root(store)?;
    let name = state_file_name(project_id)?;
    if inspect_state_for_read(root.as_raw_fd(), &name)?.is_none() {
        return Ok(());
    }
    if unsafe { libc::unlinkat(root.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != ErrorKind::NotFound {
            return Err(persistence_error("project_state_remove_failed"));
        }
    }
    if unsafe { libc::fsync(root.as_raw_fd()) } != 0 {
        return Err(persistence_error("project_state_parent_sync_failed"));
    }
    Ok(())
}

fn open_project_state_root(store: &LocalProjectStateStore) -> Result<OwnedFd> {
    let app = open_directory_path_nofollow(&store.app_private_root)?;
    let product = open_directory_at(app.as_raw_fd(), PRODUCT_ROOT_NAME)?;
    let state = open_directory_at(product.as_raw_fd(), STATE_ROOT_NAME)?;
    open_directory_at(state.as_raw_fd(), PROJECT_STATE_ROOT_NAME)
}

fn open_conversation_state_root(store: &LocalProjectStateStore) -> Result<OwnedFd> {
    let app = open_directory_path_nofollow(&store.app_private_root)?;
    let product = open_directory_at(app.as_raw_fd(), PRODUCT_ROOT_NAME)?;
    let state = open_directory_at(product.as_raw_fd(), STATE_ROOT_NAME)?;
    open_directory_at(state.as_raw_fd(), CONVERSATION_STATE_ROOT_NAME)
}

fn open_directory_path_nofollow(path: &std::path::Path) -> Result<OwnedFd> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| persistence_error("app_private_root_invalid"))?;
    let raw = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    let fd = owned_fd(raw, "app_private_root_open_failed")?;
    verify_private_directory_fd(fd.as_raw_fd(), "app_private_root_invalid")?;
    Ok(fd)
}

fn open_directory_at(parent: RawFd, name: &str) -> Result<OwnedFd> {
    let name = CString::new(name).map_err(|_| persistence_error("state_path_invalid"))?;
    let raw = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    let fd = owned_fd(raw, "state_directory_open_failed")?;
    verify_private_directory_fd(fd.as_raw_fd(), "state_directory_invalid")?;
    Ok(fd)
}

fn open_file_at(parent: RawFd, name: &CStr, access: libc::c_int) -> Result<OwnedFd> {
    let raw = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            access | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    owned_fd(raw, "project_state_open_failed")
}

fn open_exclusive_private_file_at(parent: RawFd, name: &CStr) -> Result<OwnedFd> {
    let raw = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            PRIVATE_FILE_MODE,
        )
    };
    owned_fd(raw, "project_state_temp_create_failed")
}

fn inspect_existing_state_target(parent: RawFd, name: &CStr) -> Result<()> {
    match fstatat_nofollow_optional(parent, name)? {
        None => Ok(()),
        Some(stat) => verify_private_regular_stat(&stat, "project_state_target_invalid"),
    }
}

fn inspect_state_for_read(parent: RawFd, name: &CStr) -> Result<Option<libc::stat>> {
    let Some(stat) = fstatat_nofollow_optional(parent, name)? else {
        return Ok(None);
    };
    verify_private_regular_stat(&stat, "project_state_target_invalid")?;
    Ok(Some(stat))
}

fn verify_private_directory_fd(fd: RawFd, code: &'static str) -> Result<()> {
    let stat = fstat(fd, code)?;
    if (stat.st_mode & libc::S_IFMT) != libc::S_IFDIR || stat.st_uid != unsafe { libc::geteuid() } {
        return Err(persistence_error(code));
    }
    Ok(())
}

fn verify_private_regular_fd(fd: RawFd, code: &'static str) -> Result<()> {
    let stat = fstat(fd, code)?;
    verify_private_regular_stat(&stat, code)
}

fn verify_private_regular_stat(stat: &libc::stat, code: &'static str) -> Result<()> {
    if (stat.st_mode & libc::S_IFMT) != libc::S_IFREG
        || stat.st_nlink != 1
        || stat.st_uid != unsafe { libc::geteuid() }
        || (stat.st_mode & 0o077) != 0
        || (stat.st_mode & 0o400) == 0
    {
        return Err(persistence_error(code));
    }
    Ok(())
}

fn fstat(fd: RawFd, code: &'static str) -> Result<libc::stat> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return Err(persistence_error(code));
    }
    Ok(unsafe { stat.assume_init() })
}

fn fstatat_nofollow(parent: RawFd, name: &CStr, code: &'static str) -> Result<libc::stat> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent,
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(persistence_error(code));
    }
    Ok(unsafe { stat.assume_init() })
}

fn fstatat_nofollow_optional(parent: RawFd, name: &CStr) -> Result<Option<libc::stat>> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent,
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        let error = std::io::Error::last_os_error();
        if error.kind() == ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(persistence_error("project_state_stat_failed"));
    }
    Ok(Some(unsafe { stat.assume_init() }))
}

fn state_file_name(project_id: ProjectId) -> Result<CString> {
    CString::new(format!("{}.json", project_id.0.hyphenated()))
        .map_err(|_| persistence_error("project_state_name_invalid"))
}

fn conversation_file_name(
    project_id: ProjectId,
    conversation_id: ConversationId,
) -> Result<CString> {
    CString::new(format!(
        "{}--{}.json",
        project_id.0.hyphenated(),
        conversation_id.0.hyphenated()
    ))
    .map_err(|_| persistence_error("conversation_name_invalid"))
}

fn parse_conversation_file_name(text: &str) -> Result<(ProjectId, ConversationId)> {
    let stem = text
        .strip_suffix(".json")
        .ok_or_else(|| persistence_error("conversation_directory_unexpected_entry"))?;
    let (project_text, conversation_text) = stem
        .split_once("--")
        .ok_or_else(|| persistence_error("conversation_directory_unexpected_entry"))?;
    if conversation_text.contains("--") {
        return Err(persistence_error("conversation_directory_unexpected_entry"));
    }
    let project_uuid = Uuid::parse_str(project_text)
        .map_err(|_| persistence_error("conversation_directory_unexpected_entry"))?;
    let conversation_uuid = Uuid::parse_str(conversation_text)
        .map_err(|_| persistence_error("conversation_directory_unexpected_entry"))?;
    if project_text != project_uuid.hyphenated().to_string()
        || conversation_text != conversation_uuid.hyphenated().to_string()
    {
        return Err(persistence_error("conversation_directory_unexpected_entry"));
    }
    Ok((ProjectId(project_uuid), ConversationId(conversation_uuid)))
}

fn owned_fd(raw: RawFd, code: &'static str) -> Result<OwnedFd> {
    if raw < 0 {
        return Err(persistence_error(code));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

struct TempCleanup<'a> {
    parent_fd: RawFd,
    name: &'a CStr,
    armed: bool,
}

impl Drop for TempCleanup<'_> {
    fn drop(&mut self) {
        if self.armed {
            unsafe {
                libc::unlinkat(self.parent_fd, self.name.as_ptr(), 0);
            }
        }
    }
}

#[cfg(any(target_os = "android", target_os = "linux"))]
struct DirGuard(*mut libc::DIR);

#[cfg(any(target_os = "android", target_os = "linux"))]
impl Drop for DirGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { libc::closedir(self.0) };
        }
    }
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn __errno_location() -> *mut libc::c_int;
}

#[cfg(target_os = "android")]
unsafe extern "C" {
    fn __errno() -> *mut libc::c_int;
}

#[cfg(target_os = "linux")]
fn errno_ptr() -> *mut libc::c_int {
    unsafe { __errno_location() }
}

#[cfg(target_os = "android")]
fn errno_ptr() -> *mut libc::c_int {
    unsafe { __errno() }
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn set_errno_zero() {
    unsafe { *errno_ptr() = 0 };
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn current_errno() -> i32 {
    unsafe { *errno_ptr() }
}
