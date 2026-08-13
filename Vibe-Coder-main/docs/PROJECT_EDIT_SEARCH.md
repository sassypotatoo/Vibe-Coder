# Safe project edit, patch, and search — Part 13

Part 13 builds deterministic coding primitives on top of the Part 12 fd-relative file boundary. It
still does not claim that Jcode's own built-in file/search tools are confined to these APIs.

## Exact text edit

`edit_text_file` accepts one UTF-8 `expected` fragment and one replacement. The expected fragment
must be non-empty, bounded, and have exactly one possible occurrence in the current file. Overlapping
occurrences count as ambiguity. Missing or ambiguous matches commit nothing.

The target must already be a single-link, regular, owner-writable UTF-8 file inside the managed
project. The updated file must remain within the 16 MiB write ceiling. Before the final `renameat`,
the original inode, owner mode, and complete contents are checked again; a changed target fails with
`text_edit_target_changed` rather than overwriting newer contents.

The replacement uses the same private same-directory temporary-file strategy as Part 12, including
`O_EXCL`, `O_NOFOLLOW`, a single-link temp-inode check, file sync, atomic rename, and parent sync.

## Multi-hunk patch

`apply_text_patch` accepts at most 64 exact-match hunks and at most 16 MiB of total hunk input. All
hunks are applied to one in-memory UTF-8 buffer in order. Each hunk must have exactly one match in
the buffer produced by the preceding hunk. If any hunk fails validation or matching, **zero hunks
are committed**. Only after every hunk succeeds is one atomic replacement prepared and committed.

This is intentionally not a fuzzy patch engine. VibeCoder does not guess which similar block the
model meant.

## Project file discovery

On Android/Linux, `list_project_files` starts from the freshly verified project directory fd and
enumerates with a duplicated fd + `fdopendir/readdir`. Every discovered entry is inspected with
`fstatat(..., AT_SYMLINK_NOFOLLOW)`.

The walker:

- never follows symlinks;
- skips special files and `st_nlink != 1` regular files;
- hides `.vibecoder-tmp-*` internal entries;
- returns only UTF-8 project-relative paths, never app-private absolute paths;
- sorts names/results deterministically;
- respects the public 4096-byte relative-path ceiling;
- bounds output to 4096 files, traversal to 16,384 entries, and depth to 64;
- reports skipped entries and whether traversal was truncated.

A listed path is discovery data, not durable file authority. Later reads/edits perform fresh Part 12
operation-time checks.

## Literal text search

`search_project_text` performs literal, case-sensitive UTF-8 search over safely discovered files.
It intentionally does not add regex/glob semantics yet.

Bounds:

- maximum 512 returned matches;
- maximum 4096 discovered files;
- maximum 2 MiB read per searched file;
- maximum 64 MiB total bytes scanned;
- single-line previews are bounded to 240 Unicode scalar values and control characters are removed.

Oversized, binary/non-UTF8, special, symlink, hard-linked, or concurrently-invalidated files are
skipped rather than followed. Results contain only project-relative paths plus one-based line and
column coordinates.

## Remaining boundary

These primitives reduce ambiguous edits and unsafe search traversal, but they are not process
isolation and are not a kernel-level compare-and-swap against a hostile concurrent process running
under the same Android uid. Command/process isolation remains Parts 14-15. Jcode built-in tools are
not yet proven to route exclusively through these workspace primitives.
