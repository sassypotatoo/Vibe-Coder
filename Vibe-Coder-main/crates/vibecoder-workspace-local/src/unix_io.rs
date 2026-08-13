//! Operation-time Unix/Android file primitives.
//!
//! Every operation starts from a freshly verified project-directory file descriptor and walks
//! descendants with `openat(..., O_NOFOLLOW)`. A display path returned by Part 11 is never reused as
//! authority. Regular-file reads reject multi-link inodes. Writes create a new private temporary
//! inode and `renameat` it into place instead of truncating an existing inode through a possible
//! hard-link alias.

use super::{
    INTERNAL_TEMP_PREFIX, LocalWorkspaceRuntime, MAX_COMPONENT_BYTES, MAX_FILE_READ_BYTES,
    MAX_FILE_WRITE_BYTES, MAX_PROJECT_LIST_ENTRIES, MAX_PROJECT_SEARCH_DEPTH,
    MAX_PROJECT_SEARCH_FILE_BYTES, MAX_PROJECT_SEARCH_FILES, MAX_PROJECT_SEARCH_MATCHES,
    MAX_PROJECT_SEARCH_TOTAL_BYTES, MAX_PROJECT_WALK_ENTRIES, MAX_RELATIVE_PATH_BYTES,
    MAX_TEXT_EDIT_EXPECTED_BYTES, MAX_TEXT_PATCH_HUNKS, MAX_TEXT_PATCH_INPUT_BYTES,
    PRODUCT_ROOT_NAME, PROJECTS_ROOT_NAME, validate_relative_path, workspace_error,
};
use std::ffi::{CStr, CString, OsStr};
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use uuid::Uuid;
use vibecoder_domain::{ProjectRef, Result};
use vibecoder_workspace_contract::{
    ProjectFileEntry, ProjectFileList, ProjectTextMatch, ProjectTextSearchResult, TextEditResult,
    TextPatchHunk, TextPatchResult,
};

const PRIVATE_DIR_MODE: libc::mode_t = 0o700;
const PRIVATE_FILE_MODE: libc::mode_t = 0o600;

pub(super) fn create_dir_all(
    runtime: &LocalWorkspaceRuntime,
    project: &ProjectRef,
    relative: &Path,
) -> Result<()> {
    let components = validate_relative_path(relative)?;
    let mut current = open_verified_project_root(runtime, project)?;

    for component in components {
        current = open_or_create_private_dir_at(current.as_raw_fd(), component)?;
    }
    Ok(())
}

pub(super) fn read_file(
    runtime: &LocalWorkspaceRuntime,
    project: &ProjectRef,
    relative: &Path,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    if max_bytes == 0 || max_bytes > MAX_FILE_READ_BYTES {
        return Err(workspace_error("file_read_limit_invalid"));
    }

    let components = validate_relative_path(relative)?;
    let (parent, name) = open_parent(runtime, project, &components)?;
    let expected = inspect_existing_read_target(parent.as_raw_fd(), name)?;
    if expected.st_size < 0 || expected.st_size as u64 > max_bytes as u64 {
        return Err(workspace_error("file_read_too_large"));
    }

    let fd = open_regular_file_for_read(parent.as_raw_fd(), name)?;
    let stat = fstat(fd.as_raw_fd(), "file_read_stat_failed")?;
    require_single_link_regular(
        &stat,
        "file_read_target_invalid",
        "file_hard_link_forbidden",
    )?;
    if stat.st_dev != expected.st_dev || stat.st_ino != expected.st_ino {
        return Err(workspace_error("file_changed_during_open"));
    }

    if stat.st_size < 0 || stat.st_size as u64 > max_bytes as u64 {
        return Err(workspace_error("file_read_too_large"));
    }

    let mut file: File = fd.into();
    let mut output = Vec::with_capacity((stat.st_size as usize).min(max_bytes));
    let mut bounded = (&mut file).take(max_bytes as u64 + 1);
    bounded
        .read_to_end(&mut output)
        .map_err(|_| workspace_error("file_read_failed"))?;
    if output.len() > max_bytes {
        return Err(workspace_error("file_read_too_large"));
    }
    Ok(output)
}

pub(super) fn regular_file_exists(
    runtime: &LocalWorkspaceRuntime,
    project: &ProjectRef,
    relative: &Path,
) -> Result<bool> {
    let components = validate_relative_path(relative)?;
    let (parent, name) = open_parent(runtime, project, &components)?;
    Ok(inspect_optional_read_target(parent.as_raw_fd(), name)?.is_some())
}

pub(super) fn atomic_write_file(
    runtime: &LocalWorkspaceRuntime,
    project: &ProjectRef,
    relative: &Path,
    contents: &[u8],
) -> Result<()> {
    if contents.len() > MAX_FILE_WRITE_BYTES {
        return Err(workspace_error("file_write_too_large"));
    }

    let components = validate_relative_path(relative)?;
    let (parent, name) = open_parent(runtime, project, &components)?;
    let target_mode =
        inspect_existing_write_target(parent.as_raw_fd(), name)?.unwrap_or(PRIVATE_FILE_MODE);

    let temp_name = CString::new(format!(".vibecoder-tmp-{}", Uuid::new_v4().simple()))
        .map_err(|_| workspace_error("atomic_write_temp_name_invalid"))?;
    let target_name = component_cstring(name)?;

    let temp_fd = open_exclusive_private_file_at(parent.as_raw_fd(), &temp_name, target_mode)?;
    let mut cleanup = TempCleanup {
        parent_fd: parent.as_raw_fd(),
        name: &temp_name,
        armed: true,
    };
    if unsafe { libc::fchmod(temp_fd.as_raw_fd(), target_mode) } != 0 {
        return Err(workspace_error("atomic_write_temp_mode_failed"));
    }
    let temp_stat = fstat(temp_fd.as_raw_fd(), "atomic_write_temp_stat_failed")?;
    require_single_link_regular(
        &temp_stat,
        "atomic_write_temp_invalid",
        "atomic_write_temp_hard_linked",
    )?;

    let mut temp_file: File = temp_fd.into();
    temp_file
        .write_all(contents)
        .map_err(|_| workspace_error("atomic_write_data_failed"))?;
    temp_file
        .sync_all()
        .map_err(|_| workspace_error("atomic_write_file_sync_failed"))?;
    drop(temp_file);

    let rename_result = unsafe {
        libc::renameat(
            parent.as_raw_fd(),
            temp_name.as_ptr(),
            parent.as_raw_fd(),
            target_name.as_ptr(),
        )
    };
    if rename_result != 0 {
        return Err(workspace_error("atomic_write_rename_failed"));
    }
    cleanup.armed = false;

    if unsafe { libc::fsync(parent.as_raw_fd()) } != 0 {
        return Err(workspace_error("atomic_write_parent_sync_failed"));
    }
    Ok(())
}

pub(super) fn edit_text_file(
    runtime: &LocalWorkspaceRuntime,
    project: &ProjectRef,
    relative: &Path,
    expected: &str,
    replacement: &str,
) -> Result<TextEditResult> {
    let result = apply_text_patch_pairs(runtime, project, relative, &[(expected, replacement)])?;
    Ok(TextEditResult {
        replacements: result.hunks_applied,
        bytes_before: result.bytes_before,
        bytes_after: result.bytes_after,
    })
}

pub(super) fn apply_text_patch(
    runtime: &LocalWorkspaceRuntime,
    project: &ProjectRef,
    relative: &Path,
    hunks: &[TextPatchHunk],
) -> Result<TextPatchResult> {
    if hunks.is_empty() || hunks.len() > MAX_TEXT_PATCH_HUNKS {
        return Err(workspace_error("text_patch_hunk_count_invalid"));
    }
    let pairs: Vec<(&str, &str)> = hunks
        .iter()
        .map(|hunk| (hunk.expected.as_str(), hunk.replacement.as_str()))
        .collect();
    apply_text_patch_pairs(runtime, project, relative, &pairs)
}

fn apply_text_patch_pairs(
    runtime: &LocalWorkspaceRuntime,
    project: &ProjectRef,
    relative: &Path,
    hunks: &[(&str, &str)],
) -> Result<TextPatchResult> {
    if hunks.is_empty() || hunks.len() > MAX_TEXT_PATCH_HUNKS {
        return Err(workspace_error("text_patch_hunk_count_invalid"));
    }
    let mut patch_input_bytes = 0usize;
    for &(expected, replacement) in hunks {
        if expected.is_empty() {
            return Err(workspace_error("text_edit_expected_empty"));
        }
        if expected.len() > MAX_TEXT_EDIT_EXPECTED_BYTES {
            return Err(workspace_error("text_edit_expected_too_large"));
        }
        if replacement.len() > MAX_FILE_WRITE_BYTES {
            return Err(workspace_error("text_patch_replacement_too_large"));
        }
        patch_input_bytes = patch_input_bytes
            .checked_add(expected.len())
            .and_then(|value| value.checked_add(replacement.len()))
            .ok_or_else(|| workspace_error("text_patch_input_too_large"))?;
        if patch_input_bytes > MAX_TEXT_PATCH_INPUT_BYTES {
            return Err(workspace_error("text_patch_input_too_large"));
        }
    }

    let components = validate_relative_path(relative)?;
    let (parent, name) = open_parent(runtime, project, &components)?;
    let initial_stat = inspect_existing_read_target(parent.as_raw_fd(), name)?;
    let owner_mode = initial_stat.st_mode & 0o700;
    if owner_mode & 0o200 == 0 {
        return Err(workspace_error("file_write_target_read_only"));
    }
    if initial_stat.st_size < 0 || initial_stat.st_size as u64 > MAX_FILE_WRITE_BYTES as u64 {
        return Err(workspace_error("text_edit_file_too_large"));
    }

    let original = read_verified_regular_at(
        parent.as_raw_fd(),
        name,
        &initial_stat,
        MAX_FILE_WRITE_BYTES,
        "text_edit",
    )?;
    let original_text =
        std::str::from_utf8(&original).map_err(|_| workspace_error("text_edit_file_not_utf8"))?;
    let mut updated = original_text.to_owned();

    for &(expected, replacement) in hunks {
        let match_index = find_unique_text_match(&updated, expected)?;
        let bytes_after = updated
            .len()
            .checked_sub(expected.len())
            .and_then(|value| value.checked_add(replacement.len()))
            .ok_or_else(|| workspace_error("text_edit_result_too_large"))?;
        if bytes_after > MAX_FILE_WRITE_BYTES {
            return Err(workspace_error("text_edit_result_too_large"));
        }
        updated.replace_range(match_index..match_index + expected.len(), replacement);
    }

    if updated.as_bytes() == original.as_slice() {
        return Ok(TextPatchResult {
            hunks_applied: hunks.len() as u32,
            bytes_before: original.len() as u64,
            bytes_after: updated.len() as u64,
        });
    }

    let temp_name = CString::new(format!(".vibecoder-tmp-{}", Uuid::new_v4().simple()))
        .map_err(|_| workspace_error("atomic_write_temp_name_invalid"))?;
    let target_name = component_cstring(name)?;
    let temp_fd =
        open_exclusive_private_file_at(parent.as_raw_fd(), &temp_name, owner_mode as libc::mode_t)?;
    let mut cleanup = TempCleanup {
        parent_fd: parent.as_raw_fd(),
        name: &temp_name,
        armed: true,
    };
    if unsafe { libc::fchmod(temp_fd.as_raw_fd(), owner_mode as libc::mode_t) } != 0 {
        return Err(workspace_error("text_edit_temp_mode_failed"));
    }
    let temp_stat = fstat(temp_fd.as_raw_fd(), "text_edit_temp_stat_failed")?;
    require_single_link_regular(
        &temp_stat,
        "text_edit_temp_invalid",
        "text_edit_temp_hard_linked",
    )?;

    let mut temp_file: File = temp_fd.into();
    temp_file
        .write_all(updated.as_bytes())
        .map_err(|_| workspace_error("text_edit_data_failed"))?;
    temp_file
        .sync_all()
        .map_err(|_| workspace_error("text_edit_file_sync_failed"))?;
    drop(temp_file);

    // Re-read the original inode/content immediately before replacement. This catches normal
    // concurrent VibeCoder edits and most same-uid races. Part 14/15 still own stronger process
    // isolation; this is not claimed to be a hostile same-uid kernel-level compare-and-swap.
    let current_stat = inspect_existing_read_target(parent.as_raw_fd(), name)?;
    if current_stat.st_dev != initial_stat.st_dev
        || current_stat.st_ino != initial_stat.st_ino
        || (current_stat.st_mode & 0o700) != owner_mode
    {
        return Err(workspace_error("text_edit_target_changed"));
    }
    let current = read_verified_regular_at(
        parent.as_raw_fd(),
        name,
        &current_stat,
        MAX_FILE_WRITE_BYTES,
        "text_edit_recheck",
    )?;
    if current != original {
        return Err(workspace_error("text_edit_target_changed"));
    }

    if unsafe {
        libc::renameat(
            parent.as_raw_fd(),
            temp_name.as_ptr(),
            parent.as_raw_fd(),
            target_name.as_ptr(),
        )
    } != 0
    {
        return Err(workspace_error("text_edit_rename_failed"));
    }
    cleanup.armed = false;
    if unsafe { libc::fsync(parent.as_raw_fd()) } != 0 {
        return Err(workspace_error("text_edit_parent_sync_failed"));
    }

    Ok(TextPatchResult {
        hunks_applied: hunks.len() as u32,
        bytes_before: original.len() as u64,
        bytes_after: updated.len() as u64,
    })
}

#[cfg(any(target_os = "android", target_os = "linux"))]
pub(super) fn list_project_files(
    runtime: &LocalWorkspaceRuntime,
    project: &ProjectRef,
    max_entries: usize,
) -> Result<ProjectFileList> {
    if max_entries == 0 || max_entries > MAX_PROJECT_LIST_ENTRIES {
        return Err(workspace_error("project_list_limit_invalid"));
    }
    let root = open_verified_project_root(runtime, project)?;
    let mut state = WalkState {
        files: Vec::new(),
        skipped: 0,
        entries_seen: 0,
        truncated: false,
        max_files: max_entries,
    };
    walk_project_dir(root.as_raw_fd(), "", 0, &mut state)?;
    state
        .files
        .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(ProjectFileList {
        files: state.files,
        skipped_entries: state.skipped,
        truncated: state.truncated,
    })
}

#[cfg(any(target_os = "android", target_os = "linux"))]
pub(super) fn search_project_text(
    runtime: &LocalWorkspaceRuntime,
    project: &ProjectRef,
    needle: &str,
    max_matches: usize,
) -> Result<ProjectTextSearchResult> {
    if needle.is_empty() {
        return Err(workspace_error("project_search_needle_empty"));
    }
    if needle.len() > 4096 || needle.contains('\0') {
        return Err(workspace_error("project_search_needle_invalid"));
    }
    if max_matches == 0 || max_matches > MAX_PROJECT_SEARCH_MATCHES {
        return Err(workspace_error("project_search_match_limit_invalid"));
    }

    let listing = list_project_files(runtime, project, MAX_PROJECT_SEARCH_FILES)?;
    let mut result = ProjectTextSearchResult {
        matches: Vec::new(),
        files_scanned: 0,
        files_skipped: listing.skipped_entries,
        bytes_scanned: 0,
        truncated: listing.truncated,
    };

    for entry in listing.files {
        if entry.size_bytes > MAX_PROJECT_SEARCH_FILE_BYTES as u64 {
            result.files_skipped = result.files_skipped.saturating_add(1);
            continue;
        }
        if result.bytes_scanned.saturating_add(entry.size_bytes)
            > MAX_PROJECT_SEARCH_TOTAL_BYTES as u64
        {
            result.truncated = true;
            break;
        }

        let path = Path::new(&entry.relative_path);
        let bytes = match read_file(runtime, project, path, MAX_PROJECT_SEARCH_FILE_BYTES) {
            Ok(bytes) => bytes,
            Err(_) => {
                result.files_skipped = result.files_skipped.saturating_add(1);
                continue;
            }
        };
        if result.bytes_scanned.saturating_add(bytes.len() as u64)
            > MAX_PROJECT_SEARCH_TOTAL_BYTES as u64
        {
            result.truncated = true;
            break;
        }
        result.bytes_scanned = result.bytes_scanned.saturating_add(bytes.len() as u64);
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) => text,
            Err(_) => {
                result.files_skipped = result.files_skipped.saturating_add(1);
                continue;
            }
        };
        result.files_scanned = result.files_scanned.saturating_add(1);

        for (byte_index, _) in text.match_indices(needle) {
            if result.matches.len() >= max_matches {
                result.truncated = true;
                return Ok(result);
            }
            let (line, column, preview) = locate_text_match(text, byte_index);
            result.matches.push(ProjectTextMatch {
                relative_path: entry.relative_path.clone(),
                line,
                column,
                preview,
            });
        }
    }

    Ok(result)
}

#[cfg(any(target_os = "android", target_os = "linux"))]
struct WalkState {
    files: Vec<ProjectFileEntry>,
    skipped: u32,
    entries_seen: usize,
    truncated: bool,
    max_files: usize,
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn walk_project_dir(
    dir_fd: RawFd,
    prefix: &str,
    depth: usize,
    state: &mut WalkState,
) -> Result<()> {
    if state.truncated {
        return Ok(());
    }
    if depth > MAX_PROJECT_SEARCH_DEPTH {
        state.truncated = true;
        return Ok(());
    }

    let (names, skipped_names) = read_dir_names(dir_fd)?;
    state.skipped = state.skipped.saturating_add(skipped_names);
    for name in names {
        if state.entries_seen >= MAX_PROJECT_WALK_ENTRIES {
            state.truncated = true;
            return Ok(());
        }
        state.entries_seen += 1;
        if name.starts_with(INTERNAL_TEMP_PREFIX) {
            state.skipped = state.skipped.saturating_add(1);
            continue;
        }

        let stat = match stat_entry_nofollow(dir_fd, OsStr::new(name.as_str())) {
            Ok(stat) => stat,
            Err(_) => {
                state.skipped = state.skipped.saturating_add(1);
                continue;
            }
        };
        let kind = stat.st_mode & libc::S_IFMT;
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if relative.len() > MAX_RELATIVE_PATH_BYTES {
            state.skipped = state.skipped.saturating_add(1);
            if kind == libc::S_IFDIR {
                state.truncated = true;
            }
            continue;
        }

        if kind == libc::S_IFDIR {
            if depth >= MAX_PROJECT_SEARCH_DEPTH {
                state.truncated = true;
                continue;
            }
            let child = match try_open_existing_dir_at(dir_fd, OsStr::new(name.as_str())) {
                Ok(fd) => fd,
                Err(_) => {
                    state.skipped = state.skipped.saturating_add(1);
                    state.truncated = true;
                    continue;
                }
            };
            walk_project_dir(child.as_raw_fd(), &relative, depth + 1, state)?;
            if state.truncated {
                return Ok(());
            }
        } else if kind == libc::S_IFREG {
            if stat.st_nlink != 1 || stat.st_size < 0 {
                state.skipped = state.skipped.saturating_add(1);
                continue;
            }
            if state.files.len() >= state.max_files {
                state.truncated = true;
                return Ok(());
            }
            state.files.push(ProjectFileEntry {
                relative_path: relative,
                size_bytes: stat.st_size as u64,
                owner_executable: stat.st_mode & 0o100 != 0,
            });
        } else {
            state.skipped = state.skipped.saturating_add(1);
        }
    }
    Ok(())
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn read_dir_names(dir_fd: RawFd) -> Result<(Vec<String>, u32)> {
    let duplicate = unsafe { libc::fcntl(dir_fd, libc::F_DUPFD_CLOEXEC, 0) };
    if duplicate < 0 {
        return Err(workspace_error("project_list_dir_dup_failed"));
    }
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        let _ = unsafe { libc::close(duplicate) };
        return Err(workspace_error("project_list_fdopendir_failed"));
    }
    let guard = DirStream(stream);
    let mut names = Vec::new();
    let mut skipped = 0u32;

    loop {
        set_errno_zero();
        let entry = unsafe { libc::readdir(guard.0) };
        if entry.is_null() {
            if current_errno() != 0 {
                return Err(workspace_error("project_list_readdir_failed"));
            }
            break;
        }
        let raw = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if raw == b"." || raw == b".." {
            continue;
        }
        let Ok(name) = std::str::from_utf8(raw) else {
            skipped = skipped.saturating_add(1);
            continue;
        };
        if name.is_empty()
            || name.len() > MAX_COMPONENT_BYTES
            || name.chars().any(char::is_control)
            || name.contains('\\')
        {
            skipped = skipped.saturating_add(1);
            continue;
        }
        names.push(name.to_owned());
    }
    drop(guard);
    names.sort();
    Ok((names, skipped))
}

#[cfg(any(target_os = "android", target_os = "linux"))]
struct DirStream(*mut libc::DIR);

#[cfg(any(target_os = "android", target_os = "linux"))]
impl Drop for DirStream {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let _ = unsafe { libc::closedir(self.0) };
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
fn current_errno() -> libc::c_int {
    unsafe { *errno_ptr() }
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn stat_entry_nofollow(parent_fd: RawFd, component: &OsStr) -> Result<libc::stat> {
    let name = component_cstring(component)?;
    let mut stat = MaybeUninit::<libc::stat>::zeroed();
    if unsafe {
        libc::fstatat(
            parent_fd,
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(workspace_error("project_list_entry_stat_failed"));
    }
    Ok(unsafe { stat.assume_init() })
}

fn read_verified_regular_at(
    parent_fd: RawFd,
    name: &OsStr,
    expected: &libc::stat,
    max_bytes: usize,
    error_prefix: &'static str,
) -> Result<Vec<u8>> {
    let fd = open_regular_file_for_read(parent_fd, name)?;
    let stat = fstat(fd.as_raw_fd(), "file_read_stat_failed")?;
    require_single_link_regular(
        &stat,
        "file_read_target_invalid",
        "file_hard_link_forbidden",
    )?;
    if stat.st_dev != expected.st_dev || stat.st_ino != expected.st_ino {
        return Err(workspace_error(match error_prefix {
            "text_edit" | "text_edit_recheck" => "text_edit_target_changed",
            _ => "file_changed_during_open",
        }));
    }
    if stat.st_size < 0 || stat.st_size as u64 > max_bytes as u64 {
        return Err(workspace_error("file_read_too_large"));
    }
    let mut file: File = fd.into();
    let mut output = Vec::with_capacity((stat.st_size as usize).min(max_bytes));
    let mut bounded = (&mut file).take(max_bytes as u64 + 1);
    bounded
        .read_to_end(&mut output)
        .map_err(|_| workspace_error("file_read_failed"))?;
    if output.len() > max_bytes {
        return Err(workspace_error("file_read_too_large"));
    }
    Ok(output)
}

fn find_unique_text_match(text: &str, expected: &str) -> Result<usize> {
    let first = text
        .find(expected)
        .ok_or_else(|| workspace_error("text_edit_expected_not_found"))?;
    let next_char_len = text[first..]
        .chars()
        .next()
        .map(char::len_utf8)
        .ok_or_else(|| workspace_error("text_edit_expected_not_found"))?;
    let search_from = first.saturating_add(next_char_len);
    if search_from <= text.len() && text[search_from..].contains(expected) {
        return Err(workspace_error("text_edit_expected_ambiguous"));
    }
    Ok(first)
}

fn locate_text_match(text: &str, byte_index: usize) -> (u32, u32, String) {
    let before = &text[..byte_index];
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = text[line_start..byte_index].chars().count() + 1;
    let line_end = text[byte_index..]
        .find('\n')
        .map_or(text.len(), |offset| byte_index + offset);
    let raw_preview = &text[line_start..line_end];
    let mut preview = String::new();
    for ch in raw_preview.chars().take(240) {
        if ch.is_control() {
            preview.push(' ');
        } else {
            preview.push(ch);
        }
    }
    (
        line.min(u32::MAX as usize) as u32,
        column.min(u32::MAX as usize) as u32,
        preview,
    )
}

fn open_verified_project_root(
    runtime: &LocalWorkspaceRuntime,
    project: &ProjectRef,
) -> Result<OwnedFd> {
    // First reject a forged/stale serialized ProjectRef using the Part 11 identity checks. The
    // actual operation then re-enters through directory handles rather than opening that resolved
    // PathBuf directly, so a later symlink swap in managed descendants is not followed.
    runtime.verify_project_sync(project)?;

    let app_root_path = path_cstring(runtime.app_private_root())?;
    let app_fd = unsafe {
        libc::open(
            app_root_path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    let app_fd = owned_fd(app_fd, "app_private_root_open_failed")?;

    let product_fd = open_existing_dir_at(app_fd.as_raw_fd(), OsStr::new(PRODUCT_ROOT_NAME))?;
    let projects_fd = open_existing_dir_at(product_fd.as_raw_fd(), OsStr::new(PROJECTS_ROOT_NAME))?;
    let project_name = project.id.0.hyphenated().to_string();
    let project_fd = open_existing_dir_at(projects_fd.as_raw_fd(), OsStr::new(&project_name))?;

    // Corroborate that the fd still names the path represented by this ProjectRef. If the path was
    // replaced while handles were being opened, the inode comparison fails closed.
    let metadata = fs::symlink_metadata(&project.root)
        .map_err(|_| workspace_error("project_root_metadata_failed"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(workspace_error("project_root_invalid_during_file_io"));
    }
    let stat = fstat(project_fd.as_raw_fd(), "project_root_fstat_failed")?;
    if stat.st_dev as u64 != metadata.dev() || stat.st_ino as u64 != metadata.ino() {
        return Err(workspace_error("project_root_changed_during_open"));
    }
    if (stat.st_mode & libc::S_IFMT) != libc::S_IFDIR {
        return Err(workspace_error("project_root_not_directory_during_open"));
    }
    Ok(project_fd)
}

fn open_parent<'a>(
    runtime: &LocalWorkspaceRuntime,
    project: &ProjectRef,
    components: &[&'a OsStr],
) -> Result<(OwnedFd, &'a OsStr)> {
    let (name, parents) = components
        .split_last()
        .ok_or_else(|| workspace_error("file_path_empty"))?;
    let mut current = open_verified_project_root(runtime, project)?;
    for component in parents {
        current = open_existing_dir_at(current.as_raw_fd(), component)?;
    }
    Ok((current, *name))
}

fn try_open_existing_dir_at(parent_fd: RawFd, component: &OsStr) -> io::Result<OwnedFd> {
    let name = CString::new(component.as_bytes())
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "component contains NUL"))?;
    let fd = unsafe {
        libc::openat(
            parent_fd,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` is non-negative and ownership transfers exactly once into `OwnedFd`.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn open_existing_dir_at(parent_fd: RawFd, component: &OsStr) -> Result<OwnedFd> {
    try_open_existing_dir_at(parent_fd, component)
        .map_err(|_| workspace_error("project_path_parent_not_directory_or_symlink"))
}

fn open_or_create_private_dir_at(parent_fd: RawFd, component: &OsStr) -> Result<OwnedFd> {
    match try_open_existing_dir_at(parent_fd, component) {
        Ok(fd) => return Ok(fd),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(_) => {
            return Err(workspace_error(
                "project_path_parent_not_directory_or_symlink",
            ));
        }
    }

    let name = component_cstring(component)?;
    let created = unsafe { libc::mkdirat(parent_fd, name.as_ptr(), PRIVATE_DIR_MODE) };
    if created != 0 {
        let error = io::Error::last_os_error();
        if error.kind() != ErrorKind::AlreadyExists {
            return Err(workspace_error("project_directory_create_failed"));
        }
    }
    let fd = try_open_existing_dir_at(parent_fd, component)
        .map_err(|_| workspace_error("project_directory_verify_failed"))?;
    if created == 0 && unsafe { libc::fchmod(fd.as_raw_fd(), PRIVATE_DIR_MODE) } != 0 {
        drop(fd);
        let _ = unsafe { libc::unlinkat(parent_fd, name.as_ptr(), libc::AT_REMOVEDIR) };
        return Err(workspace_error("project_directory_mode_failed"));
    }
    Ok(fd)
}

fn inspect_existing_read_target(parent_fd: RawFd, component: &OsStr) -> Result<libc::stat> {
    inspect_optional_read_target(parent_fd, component)?
        .ok_or_else(|| workspace_error("file_read_target_metadata_failed"))
}

fn inspect_optional_read_target(parent_fd: RawFd, component: &OsStr) -> Result<Option<libc::stat>> {
    let name = component_cstring(component)?;
    let mut stat = MaybeUninit::<libc::stat>::zeroed();
    if unsafe {
        libc::fstatat(
            parent_fd,
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        let error = io::Error::last_os_error();
        if error.kind() == ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(workspace_error("file_read_target_metadata_failed"));
    }
    // SAFETY: successful `fstatat` initialized the full `stat` structure.
    let stat = unsafe { stat.assume_init() };
    if (stat.st_mode & libc::S_IFMT) == libc::S_IFLNK {
        return Err(workspace_error("file_symlink_forbidden"));
    }
    require_single_link_regular(
        &stat,
        "file_read_target_invalid",
        "file_hard_link_forbidden",
    )?;
    Ok(Some(stat))
}

fn open_regular_file_for_read(parent_fd: RawFd, component: &OsStr) -> Result<OwnedFd> {
    let name = component_cstring(component)?;
    let fd = unsafe {
        libc::openat(
            parent_fd,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    owned_fd(fd, "file_read_open_failed")
}

fn inspect_existing_write_target(
    parent_fd: RawFd,
    component: &OsStr,
) -> Result<Option<libc::mode_t>> {
    let name = component_cstring(component)?;
    let mut stat = MaybeUninit::<libc::stat>::zeroed();
    let result = unsafe {
        libc::fstatat(
            parent_fd,
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        let error = io::Error::last_os_error();
        if error.kind() == ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(workspace_error("file_write_target_metadata_failed"));
    }

    // SAFETY: successful `fstatat` initialized the full `stat` structure.
    let stat = unsafe { stat.assume_init() };
    let kind = stat.st_mode & libc::S_IFMT;
    if kind == libc::S_IFLNK {
        return Err(workspace_error("file_symlink_forbidden"));
    }
    require_single_link_regular(
        &stat,
        "file_write_target_invalid",
        "file_hard_link_forbidden",
    )?;

    let owner_mode = stat.st_mode & 0o700;
    if owner_mode & 0o200 == 0 {
        return Err(workspace_error("file_write_target_read_only"));
    }
    Ok(Some(owner_mode as libc::mode_t))
}

fn open_exclusive_private_file_at(
    parent_fd: RawFd,
    name: &CString,
    mode: libc::mode_t,
) -> Result<OwnedFd> {
    let fd = unsafe {
        libc::openat(
            parent_fd,
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            mode,
        )
    };
    owned_fd(fd, "atomic_write_temp_create_failed")
}

fn require_single_link_regular(
    stat: &libc::stat,
    invalid_code: &'static str,
    hard_link_code: &'static str,
) -> Result<()> {
    if (stat.st_mode & libc::S_IFMT) != libc::S_IFREG {
        return Err(workspace_error(invalid_code));
    }
    if stat.st_nlink != 1 {
        return Err(workspace_error(hard_link_code));
    }
    Ok(())
}

fn fstat(fd: RawFd, error_code: &'static str) -> Result<libc::stat> {
    let mut stat = MaybeUninit::<libc::stat>::zeroed();
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return Err(workspace_error(error_code));
    }
    // SAFETY: successful `fstat` initialized the full `stat` structure.
    Ok(unsafe { stat.assume_init() })
}

fn owned_fd(fd: libc::c_int, error_code: &'static str) -> Result<OwnedFd> {
    if fd < 0 {
        return Err(workspace_error(error_code));
    }
    // SAFETY: `fd` is non-negative and ownership transfers exactly once into `OwnedFd`.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn component_cstring(component: &OsStr) -> Result<CString> {
    CString::new(component.as_bytes()).map_err(|_| workspace_error("project_path_contains_nul"))
}

fn path_cstring(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| workspace_error("project_root_contains_nul"))
}

struct TempCleanup<'a> {
    parent_fd: RawFd,
    name: &'a CString,
    armed: bool,
}

impl Drop for TempCleanup<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = unsafe { libc::unlinkat(self.parent_fd, self.name.as_ptr(), 0) };
        }
    }
}
