# Phone-local workspace roots and canonical containment — Parts 11–12

Part 11 added the managed Android-local root and logical containment resolver. Part 12 adds operation-time safe file primitives for the Unix/Android target.

## Trust anchor

The Android/platform layer supplies one already-existing app-private directory (the equivalent of an
Android `filesDir`/private data location) to `LocalWorkspaceRuntime::initialize`.

Requirements:

- the supplied path is absolute;
- the final supplied path itself is not a symlink;
- it exists and canonicalizes to a directory.

VibeCoder then creates only fixed-name descendants:

```text
<canonical app-private dir>/
└── vibecoder/
    └── projects/
        └── <hyphenated ProjectId UUID>/
```

The fixed `vibecoder` and `projects` directories are rejected if they already exist as symlinks or
non-directories. Their canonical parent must remain the expected canonical parent.

## Project-root authority

A caller/model no longer supplies `WorkspaceSpec.root`.

`WorkspaceSpec` owns a fresh private `ProjectId` and is not serializable/caller-constructible with an
arbitrary id. The local runtime derives the physical directory from that identity. Re-opening an existing project is explicitly id-based. A serialized/tampered `ProjectRef` must match the
exact expected path for its id and must canonicalize to that same managed root before use.

This prevents a request such as `/sdcard`, `/data/local/tmp`, or another app-private path from becoming
a project merely because a model/caller asked for it.

## Relative path resolution

`resolve_project_path(project, relative)` accepts project-relative UTF-8 paths only. It rejects:

- absolute/rooted paths;
- `..` components;
- backslashes (to avoid cross-platform separator ambiguity);
- control characters (including NUL/newline/tab);
- overlong relative paths/components;
- an existing symlink at any traversed component;
- an existing intermediate component that is not a directory;
- any existing component whose canonical location escapes the verified project root.

Non-existent descendants are allowed so Part 12 can create new files/directories, but their nearest
existing ancestry must have already passed the same managed-root verification.

## Important Part 11 boundary

This resolver is **not a durable authorization token**. Filesystem state can change after a path is
resolved. Part 12 now performs operation-time directory-handle-relative checks/opens and does not accept a previously resolved `PathBuf` as proof that containment is still true.

Likewise, command/process isolation is not claimed here. A future shell process runs with the app uid
unless later sandboxing constrains it, so Part 14/15 must not treat this logical path resolver as an OS
**not a process sandbox** boundary.

## Capabilities

The Part 11 local workspace advertises:

- `managed_project_roots = true`
- `canonical_path_containment = true`
- `read_write_files = true` on Unix/Android
- `commands = false`
- `process_isolation = false`
- `resource_limits = false`
- `snapshots = false`

The remaining false values are intentional capability truth, not missing documentation.

## Hard-link note for Part 12

Canonical path containment cannot prove that an existing regular file is not a hard-link alias of an inode reachable elsewhere under the same app uid. Part 12 atomic-write/file-open policy must therefore avoid write-through semantics and, on the Android/Unix target, consider rejecting suspicious multi-link regular files. Later command isolation must also prevent an agent shell from manufacturing aliases to runtime/private files outside its project.

## Part 12 operation-time file primitives

See `docs/SAFE_FILE_IO.md`. Reads and writes re-enter through the app-private managed directory tree with `openat`/`O_NOFOLLOW`; regular-file reads reject hard-link aliases, directory creation verifies each newly materialized component, and writes use a private same-directory temp inode followed by `renameat` rather than in-place truncation.

The Part 12 file-I/O capability is a workspace primitive only. It is not a claim that Jcode built-in tools or later shell processes are already OS-sandboxed.

## Part 13 discovery/edit boundary

Project file discovery and literal search now enumerate from verified directory handles and return
project-relative observations only. They never turn a listed path into durable authority. Exact
edits and multi-hunk patches commit through the Part 12 atomic replacement boundary and fail closed
on missing/ambiguous matches or detected target changes. Jcode built-in tool confinement remains a
later integration boundary.
