use crate::{ProbeState, RuntimeComponentEvidence};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
#[cfg(target_os = "android")]
use std::io::ErrorKind;
#[cfg(target_os = "android")]
use std::os::unix::io::AsRawFd;
#[cfg(target_os = "android")]
use std::process::{Command, Stdio};
#[cfg(target_os = "android")]
use std::thread;
#[cfg(target_os = "android")]
use std::time::{Duration, Instant};

const ELF_HEADER_BYTES: usize = 64;
const ELF64_PROGRAM_HEADER_BYTES: usize = 56;
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const ET_DYN: u16 = 3;
const EM_AARCH64: u16 = 183;
const PT_LOAD: u32 = 1;
const PT_INTERP: u32 = 3;
const MAX_INTERPRETER_BYTES: u64 = 256;
const ANDROID_LINKER64: &str = "/system/bin/linker64";
const MAX_PROGRAM_HEADERS: u16 = 256;
#[cfg(target_os = "android")]
const MAX_PROBE_OUTPUT_BYTES: u64 = 8 * 1024;
#[cfg(target_os = "android")]
const MAX_VERSION_PROBE_TIMEOUT_MS: u64 = 10_000;
const REQUIRED_PAGE_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeArtifactProbe {
    pub package_presence: ProbeState,
    pub arm64_identity: ProbeState,
    pub execution: ProbeState,
    pub version: ProbeState,
    pub page_size_16k_compatibility: ProbeState,
    pub observed_version: Option<String>,
}

impl NativeArtifactProbe {
    pub fn into_component_evidence(
        self,
        component_id: impl Into<String>,
        expected_version_requirement: Option<&str>,
    ) -> RuntimeComponentEvidence {
        RuntimeComponentEvidence {
            component_id: component_id.into(),
            package_presence: self.package_presence,
            arm64_identity: self.arm64_identity,
            execution: self.execution,
            version: self.version,
            unix_socket_round_trip: ProbeState::NotRun,
            service_round_trip: ProbeState::NotRun,
            runtime_binding: ProbeState::NotRun,
            page_size_16k_compatibility: self.page_size_16k_compatibility,
            expected_version_requirement: expected_version_requirement.map(str::to_owned),
            observed_version: self.observed_version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ElfInspection {
    arm64: bool,
    page_16k: bool,
    interpreter: Option<String>,
}

/// Inspect one package-installed native artifact without executing it.
///
/// The parser is intentionally narrow: Android VibeCoder artifacts must be little-endian ELF64
/// AArch64. 16 KB compatibility is accepted only when every PT_LOAD segment advertises at least
/// 16 KB alignment and its file/virtual offsets are congruent at the 16 KB boundary.
pub fn probe_android_native_artifact(path: &Path) -> NativeArtifactProbe {
    let mut probe = NativeArtifactProbe {
        package_presence: ProbeState::Failed,
        arm64_identity: ProbeState::NotRun,
        execution: ProbeState::NotRun,
        version: ProbeState::NotRun,
        page_size_16k_compatibility: ProbeState::NotRun,
        observed_version: None,
    };

    let Ok(metadata) = fs::symlink_metadata(path) else {
        return probe;
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return probe;
    }
    probe.package_presence = ProbeState::Passed;

    match inspect_elf(path) {
        Ok(inspection) => {
            probe.arm64_identity = state(inspection.arm64);
            probe.page_size_16k_compatibility = state(inspection.page_16k);
        }
        Err(()) => {
            probe.arm64_identity = ProbeState::Failed;
            probe.page_size_16k_compatibility = ProbeState::Failed;
        }
    }
    probe
}

/// Probe an Android package-installed native executable.
///
/// On non-Android hosts the file is still structurally inspected but execution is deliberately
/// left `NotRun`; an x86_64 CI runner cannot attest that an AArch64 Android binary really executes.
/// On Android the supplied version arguments are executed with a bounded timeout/output budget.
pub fn probe_android_native_executable(
    path: &Path,
    version_args: &[&str],
    version_requirement: &str,
    timeout_ms: u64,
) -> NativeArtifactProbe {
    let mut probe = probe_android_native_artifact(path);
    if probe.package_presence != ProbeState::Passed
        || probe.arm64_identity != ProbeState::Passed
        || probe.page_size_16k_compatibility != ProbeState::Passed
    {
        return probe;
    }

    // A generic Linux/AArch64 PIE is not automatically an Android executable. Reject a foreign
    // PT_INTERP before attempting exec so a glibc release cannot accidentally be accepted merely
    // because its machine field and page alignment look correct. Static PIE may omit PT_INTERP.
    match inspect_elf(path) {
        Ok(inspection)
            if inspection
                .interpreter
                .as_deref()
                .is_none_or(|value| value == ANDROID_LINKER64) => {}
        Ok(_) | Err(()) => {
            probe.arm64_identity = ProbeState::Failed;
            return probe;
        }
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = (version_args, version_requirement, timeout_ms);
        return probe;
    }

    #[cfg(target_os = "android")]
    {
        let mut probe = probe;
        let timeout_ms = timeout_ms.clamp(1, MAX_VERSION_PROBE_TIMEOUT_MS);
        match run_bounded_command(path, version_args, Duration::from_millis(timeout_ms)) {
            ProbeCommandResult::Exited { success, output } => {
                probe.execution = state(success);
                if success {
                    let output = String::from_utf8_lossy(&output).trim().to_owned();
                    probe.observed_version = first_semver_triplet(&output).map(format_semver);
                    probe.version = state(version_requirement_matches(version_requirement, &output));
                } else {
                    probe.version = ProbeState::Failed;
                }
            }
            ProbeCommandResult::TimedOut | ProbeCommandResult::SpawnFailed => {
                probe.execution = ProbeState::Failed;
                probe.version = ProbeState::Failed;
            }
        }
        probe
    }
}

fn inspect_elf(path: &Path) -> Result<ElfInspection, ()> {
    let mut file = File::open(path).map_err(|_| ())?;
    let mut header = [0_u8; ELF_HEADER_BYTES];
    file.read_exact(&mut header).map_err(|_| ())?;
    if &header[0..4] != b"\x7fELF" || header[4] != ELFCLASS64 || header[5] != ELFDATA2LSB {
        return Err(());
    }

    let elf_type = read_u16(&header[16..18]);
    let machine = read_u16(&header[18..20]);
    if elf_type != ET_DYN {
        return Err(());
    }
    let phoff = read_u64(&header[32..40]);
    let phentsize = read_u16(&header[54..56]);
    let phnum = read_u16(&header[56..58]);
    if phentsize < ELF64_PROGRAM_HEADER_BYTES as u16 || phnum == 0 || phnum > MAX_PROGRAM_HEADERS {
        return Err(());
    }

    let file_len = file.metadata().map_err(|_| ())?.len();
    let table_bytes = u64::from(phentsize).checked_mul(u64::from(phnum)).ok_or(())?;
    let table_end = phoff.checked_add(table_bytes).ok_or(())?;
    if phoff < ELF_HEADER_BYTES as u64 || table_end > file_len {
        return Err(());
    }

    let mut saw_load = false;
    let mut page_16k = true;
    let mut interpreter_range: Option<(u64, u64)> = None;
    for index in 0..phnum {
        let offset = phoff
            .checked_add(u64::from(index).checked_mul(u64::from(phentsize)).ok_or(())?)
            .ok_or(())?;
        file.seek(SeekFrom::Start(offset)).map_err(|_| ())?;
        let mut ph = [0_u8; ELF64_PROGRAM_HEADER_BYTES];
        file.read_exact(&mut ph).map_err(|_| ())?;
        let header_type = read_u32(&ph[0..4]);
        let file_offset = read_u64(&ph[8..16]);
        let virtual_address = read_u64(&ph[16..24]);
        let file_size = read_u64(&ph[32..40]);
        let memory_size = read_u64(&ph[40..48]);
        let align = read_u64(&ph[48..56]);
        if header_type == PT_INTERP {
            if interpreter_range.is_some()
                || file_size < 2
                || file_size > MAX_INTERPRETER_BYTES
                || file_offset
                    .checked_add(file_size)
                    .is_none_or(|segment_end| segment_end > file_len)
            {
                return Err(());
            }
            interpreter_range = Some((file_offset, file_size));
            continue;
        }
        if header_type != PT_LOAD {
            continue;
        }
        saw_load = true;
        if file_size > memory_size
            || file_offset
                .checked_add(file_size)
                .is_none_or(|segment_end| segment_end > file_len)
        {
            return Err(());
        }
        if align < REQUIRED_PAGE_BYTES
            || !align.is_power_of_two()
            || file_offset % REQUIRED_PAGE_BYTES != virtual_address % REQUIRED_PAGE_BYTES
        {
            page_16k = false;
        }
    }
    if !saw_load {
        return Err(());
    }

    let interpreter = match interpreter_range {
        None => None,
        Some((offset, size)) => {
            file.seek(SeekFrom::Start(offset)).map_err(|_| ())?;
            let mut bytes = vec![0_u8; usize::try_from(size).map_err(|_| ())?];
            file.read_exact(&mut bytes).map_err(|_| ())?;
            if bytes.last().copied() != Some(0) {
                return Err(());
            }
            bytes.pop();
            let value = String::from_utf8(bytes).map_err(|_| ())?;
            if value.is_empty() {
                return Err(());
            }
            Some(value)
        }
    };

    Ok(ElfInspection {
        arm64: machine == EM_AARCH64,
        page_16k,
        interpreter,
    })
}

fn read_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn read_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

fn state(value: bool) -> ProbeState {
    if value {
        ProbeState::Passed
    } else {
        ProbeState::Failed
    }
}

#[cfg(target_os = "android")]
enum ProbeCommandResult {
    Exited { success: bool, output: Vec<u8> },
    TimedOut,
    SpawnFailed,
}

#[cfg(target_os = "android")]
fn run_bounded_command(path: &Path, args: &[&str], timeout: Duration) -> ProbeCommandResult {
    let mut child = match Command::new(path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return ProbeCommandResult::SpawnFailed,
    };

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    if stdout.as_ref().is_some_and(|pipe| !set_nonblocking(pipe))
        || stderr.as_ref().is_some_and(|pipe| !set_nonblocking(pipe))
    {
        let _ = child.kill();
        let _ = child.wait();
        return ProbeCommandResult::SpawnFailed;
    }

    let deadline = Instant::now() + timeout;
    let mut output = Vec::new();
    loop {
        if let Some(pipe) = stdout.as_mut() {
            drain_nonblocking(pipe, &mut output);
        }
        if let Some(pipe) = stderr.as_mut() {
            drain_nonblocking(pipe, &mut output);
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                // One final nonblocking drain captures bytes already delivered by the direct child.
                // We intentionally do not wait for EOF because a descendant may inherit a pipe.
                if let Some(pipe) = stdout.as_mut() {
                    drain_nonblocking(pipe, &mut output);
                }
                if let Some(pipe) = stderr.as_mut() {
                    drain_nonblocking(pipe, &mut output);
                }
                return ProbeCommandResult::Exited {
                    success: status.success(),
                    output,
                };
            }
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return ProbeCommandResult::TimedOut;
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return ProbeCommandResult::SpawnFailed;
            }
        }
    }
}

#[cfg(target_os = "android")]
fn set_nonblocking<T: AsRawFd>(pipe: &T) -> bool {
    let fd = pipe.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == 0
}

#[cfg(target_os = "android")]
fn drain_nonblocking<R: Read>(pipe: &mut R, output: &mut Vec<u8>) {
    let mut chunk = [0_u8; 1024];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => return,
            Ok(read) => {
                let remaining = (MAX_PROBE_OUTPUT_BYTES as usize).saturating_sub(output.len());
                if remaining > 0 {
                    output.extend_from_slice(&chunk[..read.min(remaining)]);
                }
                // Keep draining even after the capture cap so a chatty --version process cannot
                // deadlock on a full pipe before reaching its exit status.
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => return,
            Err(_) => return,
        }
    }
}

pub fn version_requirement_is_supported(requirement: &str) -> bool {
    let requirement = requirement.trim();
    if exact_semver(requirement).is_some() {
        return true;
    }
    let mut saw_branch = false;
    for branch in requirement.split("||") {
        let branch = branch.trim();
        if branch.is_empty() {
            return false;
        }
        saw_branch = true;
        let mut saw_comparator = false;
        for token in branch.split_whitespace() {
            if parse_comparator(token).is_none() {
                return false;
            }
            saw_comparator = true;
        }
        if !saw_comparator {
            return false;
        }
    }
    saw_branch
}

fn version_requirement_matches(requirement: &str, output: &str) -> bool {
    let Some(actual) = first_semver_triplet(output) else {
        return false;
    };
    let requirement = requirement.trim();
    if !version_requirement_is_supported(requirement) {
        return false;
    }
    if let Some(expected) = exact_semver(requirement) {
        return expected == actual;
    }

    // Supported range grammar is intentionally small and deterministic: OR branches separated by
    // `||`, with whitespace-separated comparators inside each branch. This covers the runtime
    // inventory without accepting arbitrary npm-semver syntax that the Android probe cannot prove.
    for branch in requirement.split("||") {
        let branch = branch.trim();
        if branch.is_empty() {
            return false;
        }
        let mut saw_comparator = false;
        let mut branch_matches = true;
        for token in branch.split_whitespace() {
            let Some((operator, expected)) = parse_comparator(token) else {
                return false;
            };
            saw_comparator = true;
            branch_matches &= match operator {
                Comparison::Less => actual < expected,
                Comparison::LessOrEqual => actual <= expected,
                Comparison::Greater => actual > expected,
                Comparison::GreaterOrEqual => actual >= expected,
                Comparison::Equal => actual == expected,
            };
        }
        if !saw_comparator {
            return false;
        }
        if branch_matches {
            return true;
        }
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Comparison {
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    Equal,
}

fn parse_comparator(token: &str) -> Option<(Comparison, (u64, u64, u64))> {
    let (operator, version) = if let Some(value) = token.strip_prefix(">=") {
        (Comparison::GreaterOrEqual, value)
    } else if let Some(value) = token.strip_prefix("<=") {
        (Comparison::LessOrEqual, value)
    } else if let Some(value) = token.strip_prefix('>') {
        (Comparison::Greater, value)
    } else if let Some(value) = token.strip_prefix('<') {
        (Comparison::Less, value)
    } else if let Some(value) = token.strip_prefix('=') {
        (Comparison::Equal, value)
    } else {
        return None;
    };
    Some((operator, semver_bound(version)?))
}

fn semver_bound(value: &str) -> Option<(u64, u64, u64)> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.') {
        return None;
    }
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|part| part.is_empty()) {
        return None;
    }
    let major = parts[0].parse().ok()?;
    let minor = parts.get(1).map_or(Some(0), |part| part.parse().ok())?;
    let patch = parts.get(2).map_or(Some(0), |part| part.parse().ok())?;
    Some((major, minor, patch))
}

fn exact_semver(value: &str) -> Option<(u64, u64, u64)> {
    if value.matches('.').count() != 2 {
        return None;
    }
    semver_bound(value)
}

fn first_semver_triplet(value: &str) -> Option<(u64, u64, u64)> {
    let bytes = value.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        if !bytes[start].is_ascii_digit() {
            start += 1;
            continue;
        }
        let mut end = start;
        while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
            end += 1;
        }
        let candidate = &value[start..end];
        if let Some(version) = exact_semver(candidate.trim_end_matches('.')) {
            return Some(version);
        }
        start = end.saturating_add(1);
    }
    None
}

fn format_semver(version: (u64, u64, u64)) -> String {
    format!("{}.{}.{}", version.0, version.1, version.2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn synthetic_elf(machine: u16, align: u64, offset: u64, vaddr: u64) -> Vec<u8> {
        let mut bytes = vec![0_u8; ELF_HEADER_BYTES + ELF64_PROGRAM_HEADER_BYTES];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = ELFCLASS64;
        bytes[5] = ELFDATA2LSB;
        bytes[16..18].copy_from_slice(&ET_DYN.to_le_bytes());
        bytes[18..20].copy_from_slice(&machine.to_le_bytes());
        bytes[32..40].copy_from_slice(&(ELF_HEADER_BYTES as u64).to_le_bytes());
        bytes[54..56].copy_from_slice(&(ELF64_PROGRAM_HEADER_BYTES as u16).to_le_bytes());
        bytes[56..58].copy_from_slice(&1_u16.to_le_bytes());
        let ph = ELF_HEADER_BYTES;
        bytes[ph..ph + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        bytes[ph + 8..ph + 16].copy_from_slice(&offset.to_le_bytes());
        bytes[ph + 16..ph + 24].copy_from_slice(&vaddr.to_le_bytes());
        bytes[ph + 48..ph + 56].copy_from_slice(&align.to_le_bytes());
        bytes
    }

    fn synthetic_elf_with_interpreter(interpreter: &str) -> Vec<u8> {
        let phoff = ELF_HEADER_BYTES;
        let phnum = 2_usize;
        let interp_offset = phoff + phnum * ELF64_PROGRAM_HEADER_BYTES;
        let interp = format!("{interpreter}\0").into_bytes();
        let mut bytes = vec![0_u8; interp_offset + interp.len()];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = ELFCLASS64;
        bytes[5] = ELFDATA2LSB;
        bytes[16..18].copy_from_slice(&ET_DYN.to_le_bytes());
        bytes[18..20].copy_from_slice(&EM_AARCH64.to_le_bytes());
        bytes[32..40].copy_from_slice(&(phoff as u64).to_le_bytes());
        bytes[54..56].copy_from_slice(&(ELF64_PROGRAM_HEADER_BYTES as u16).to_le_bytes());
        bytes[56..58].copy_from_slice(&(phnum as u16).to_le_bytes());

        let load = phoff;
        bytes[load..load + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        bytes[load + 48..load + 56].copy_from_slice(&REQUIRED_PAGE_BYTES.to_le_bytes());

        let interp_ph = phoff + ELF64_PROGRAM_HEADER_BYTES;
        bytes[interp_ph..interp_ph + 4].copy_from_slice(&PT_INTERP.to_le_bytes());
        bytes[interp_ph + 8..interp_ph + 16].copy_from_slice(&(interp_offset as u64).to_le_bytes());
        bytes[interp_ph + 32..interp_ph + 40].copy_from_slice(&(interp.len() as u64).to_le_bytes());
        bytes[interp_ph + 40..interp_ph + 48].copy_from_slice(&(interp.len() as u64).to_le_bytes());
        bytes[interp_offset..].copy_from_slice(&interp);
        bytes
    }

    #[test]
    fn parser_accepts_aarch64_with_16k_load_alignment() {
        let mut path = std::env::temp_dir();
        path.push(format!("vibecoder-part27-elf-{}", std::process::id()));
        let mut file = File::create(&path).expect("temp elf");
        file.write_all(&synthetic_elf(EM_AARCH64, 16 * 1024, 0, 0))
            .expect("write elf");
        drop(file);
        let probe = probe_android_native_artifact(&path);
        let _ = fs::remove_file(&path);
        assert_eq!(probe.package_presence, ProbeState::Passed);
        assert_eq!(probe.arm64_identity, ProbeState::Passed);
        assert_eq!(probe.page_size_16k_compatibility, ProbeState::Passed);
        assert_eq!(probe.execution, ProbeState::NotRun);
    }


    #[test]
    fn executable_probe_accepts_android_linker_identity_without_executing_on_host() {
        let mut path = std::env::temp_dir();
        path.push(format!("vibecoder-part29-android-linker-{}", std::process::id()));
        fs::write(&path, synthetic_elf_with_interpreter(ANDROID_LINKER64)).expect("write elf");
        let inspection = inspect_elf(&path).expect("inspect elf");
        let probe = probe_android_native_executable(&path, &["--version"], "0.73.0", 100);
        let _ = fs::remove_file(&path);
        assert_eq!(inspection.interpreter.as_deref(), Some(ANDROID_LINKER64));
        assert_eq!(probe.arm64_identity, ProbeState::Passed);
        assert_eq!(probe.execution, ProbeState::NotRun);
    }

    #[test]
    fn executable_probe_rejects_glibc_aarch64_interpreter_before_exec() {
        let mut path = std::env::temp_dir();
        path.push(format!("vibecoder-part29-glibc-linker-{}", std::process::id()));
        fs::write(
            &path,
            synthetic_elf_with_interpreter("/lib/ld-linux-aarch64.so.1"),
        )
        .expect("write elf");
        let probe = probe_android_native_executable(&path, &["--version"], "0.73.0", 100);
        let _ = fs::remove_file(&path);
        assert_eq!(probe.package_presence, ProbeState::Passed);
        assert_eq!(probe.arm64_identity, ProbeState::Failed);
        assert_eq!(probe.execution, ProbeState::NotRun);
    }

    #[test]
    fn parser_rejects_4k_only_load_alignment() {
        let mut path = std::env::temp_dir();
        path.push(format!("vibecoder-part27-elf-4k-{}", std::process::id()));
        fs::write(&path, synthetic_elf(EM_AARCH64, 4 * 1024, 0, 0)).expect("write elf");
        let probe = probe_android_native_artifact(&path);
        let _ = fs::remove_file(&path);
        assert_eq!(probe.arm64_identity, ProbeState::Passed);
        assert_eq!(probe.page_size_16k_compatibility, ProbeState::Failed);
    }

    #[test]
    fn parser_rejects_non_pie_or_shared_object_type() {
        let mut bytes = synthetic_elf(EM_AARCH64, 16 * 1024, 0, 0);
        bytes[16..18].copy_from_slice(&2_u16.to_le_bytes());
        let mut path = std::env::temp_dir();
        path.push(format!("vibecoder-part27-elf-exec-{}", std::process::id()));
        fs::write(&path, bytes).expect("write elf");
        let probe = probe_android_native_artifact(&path);
        let _ = fs::remove_file(&path);
        assert_eq!(probe.arm64_identity, ProbeState::Failed);
    }

    #[test]
    fn version_requirement_supports_exact_and_bounded_or_ranges() {
        assert!(version_requirement_matches("24.19.0", "node v24.19.0"));
        assert!(!version_requirement_matches("24.19.0", "node v24.19.1"));
        assert!(version_requirement_matches(
            ">=22.22.2 <23 || >=24.0.0 <27",
            "v24.19.1",
        ));
        assert!(version_requirement_matches(
            ">=22.22.2 <23 || >=24.0.0 <27",
            "v22.22.2",
        ));
        assert!(!version_requirement_matches(
            ">=22.22.2 <23 || >=24.0.0 <27",
            "v23.5.0",
        ));
    }

    #[test]
    fn version_requirement_rejects_unstructured_placeholder_text() {
        assert!(!version_requirement_matches(
            "Android ARM64-compatible JDK; exact distribution/version not yet pinned",
            "java 21.0.8",
        ));
        assert!(!version_requirement_matches("24.x", "v24.19.0"));
        assert!(version_requirement_is_supported("0.73.0"));
        assert!(version_requirement_is_supported(
            ">=22.22.2 <23 || >=24.0.0 <27",
        ));
        assert!(!version_requirement_is_supported("android-arm64-compatible-jdk"));
    }

    #[test]
    fn parser_rejects_wrong_architecture() {
        let mut path = std::env::temp_dir();
        path.push(format!("vibecoder-part27-elf-x86-{}", std::process::id()));
        fs::write(&path, synthetic_elf(62, 16 * 1024, 0, 0)).expect("write elf");
        let probe = probe_android_native_artifact(&path);
        let _ = fs::remove_file(&path);
        assert_eq!(probe.arm64_identity, ProbeState::Failed);
    }
}
