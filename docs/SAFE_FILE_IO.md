# Safe phone-local file I/O — Part 12

Part 12 adds the first file mutation/read primitives to the managed Android-local workspace. The
security target is Android/Linux/Unix. It does not pretend that a canonical `PathBuf` from Part 11
remains safe forever.

## Operation-time root entry

Every file operation first re-verifies the serialized `ProjectRef`, then re-enters the managed tree
through directory handles:

```text
<app-private root fd>
  -> vibecoder      (openat + O_DIRECTORY + O_NOFOLLOW)
  -> projects       (openat + O_DIRECTORY + O_NOFOLLOW)
  -> <ProjectId>    (openat + O_DIRECTORY + O_NOFOLLOW)
  -> descendants    (openat relative to the previous directory fd)
```

The final project fd is corroborated against the current project path inode. Existing symlinked
parents and non-directory parents fail closed. Absolute paths, `..`, backslash ambiguity, control
characters, oversized paths, and other Part 11 path violations remain rejected.

## Directory creation

`create_dir_all` walks one validated relative component at a time. A missing component is created
with `mkdirat` and then re-opened with `O_NOFOLLOW`; a concurrent `EEXIST` is accepted only if the
new object then verifies as a real directory. Directories created by VibeCoder are forced to owner
mode `0700`.

## Bounded reads

`read_file`:

- requires a non-empty project-relative path;
- preflights the final object with `fstatat(... AT_SYMLINK_NOFOLLOW)` and accepts regular files only;
- opens it with `openat(... O_NOFOLLOW | O_NONBLOCK)`, then requires the opened device/inode to match the preflight;
- rejects `st_nlink != 1` hard-link aliases;
- applies both a caller limit and a 16 MiB runtime hard limit;
- performs a second bounded read check in case the file grows after `fstat`.

The hard-link rule is intentionally conservative: a project path must not become a read alias to
other app-private data under the same Android uid.

## Atomic writes

`atomic_write_file` never opens an existing target with truncate/write-through semantics.

1. Parent directories are opened from the verified project fd with `O_NOFOLLOW`.
2. Existing targets must be single-link regular files and owner-writable. Symlinks, special files,
   directories, suspicious hard links, and owner-read-only files fail closed.
3. A unique `.vibecoder-tmp-<uuid>` regular file is created in the same parent with `O_EXCL` and
   `O_NOFOLLOW`.
4. New files use `0600`. For an existing file, only owner mode bits are preserved so an executable
   owner script remains executable while group/other access is stripped.
5. Contents are written and the temp file is `fsync`ed.
6. `renameat` atomically replaces the directory entry in the same directory.
7. The parent directory is `fsync`ed for rename durability.
8. Pre-rename failures best-effort unlink the temporary inode.

The `.vibecoder-tmp-` prefix is reserved for this internal mechanism and is rejected from public project-relative path requests.

Because replacement happens by rename rather than truncating the old inode, an attacker cannot use
an existing hard-link alias to make the write mutate the other link. We reject multi-link targets
anyway because their presence is suspicious.

## Current boundary

`read_write_files = true` means the **workspace runtime primitives** now exist on Unix/Android. It
does **not** yet mean every Jcode built-in file tool is forced through these primitives. The Jcode
tool-confinement/command-isolation boundary is later integration work and must not be inferred from
this capability.

Likewise, fd-relative traversal greatly reduces path/symlink check-use races, but Part 12 does not
claim security against an untrusted concurrent process running under the same Android uid that can
rename already-open directories. Command execution remains disabled until later process-policy and
isolation stages close that broader same-uid mutation boundary.

Non-Unix targets fail closed with `secure_file_io_unsupported_platform`; no insecure portable
fallback is advertised.

## Part 13 extension

Part 13 layers exact text edits, all-or-nothing multi-hunk patches, fd-based project discovery, and
bounded literal text search on this boundary. Search/list results are not durable authorization;
subsequent reads/edits re-run the operation-time checks described above. See
`docs/PROJECT_EDIT_SEARCH.md`.
