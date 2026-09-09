# Native configuration creation

2026-09-09. [`ConfigCreation`](../../crates/harness-core/src/config_create.rs)
adds an absent ordinary configuration file to
[`Registration::apply_with_files`](../../crates/harness-core/src/registration.rs).
Existing-file changes, new files and source links share one immutable schema-5
intent and its bound completion marker. This extends the earlier
[existing-file publication](rust-config-file.md); old native intent formats are
preserved and refused, not automatically migrated.

New files are created exclusively in a sibling transaction. Their exact object
identity, candidate bytes and requested paths enter the journal before creation
commits. Publication then renames the verified object without replacing an
existing destination. Conflict checks include aliases and destination/state
overlaps, repeated using retained staging-parent handles before intent is written.
Normalized names serve conflict checks only; requested paths remain the authority
for mutations. Disconnect removes only matching created objects and restores
recorded existing files. All configuration conflicts are checked before the first
undo mutation; an unresolved foreign edit retains the journal.

The implementation uses the existing guarded local NTFS/TxF boundary.
[Exclusive transacted creation](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createfiletransactedw)
fails on an existing name. Unsupported protection fails closed; no claim is made
for network filesystems, ReFS or a complete native installer.

A real regression test found that renaming a staged ordinary file into a recently
deleted destination changed its creation time while retaining its file ID.
Microsoft describes this name-metadata inheritance as
[NTFS tunneling](https://devblogs.microsoft.com/oldnewthing/20050715-14/?p=34923).
The old identity check would then prevent recovery of the newly published file.
Publication now restores the journaled creation time through the same transaction
handle before commit, then verifies identity. The
[SetFileTime contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfiletime)
requires attribute-write access; its change participates in the bound transaction
according to the [TxF contract](https://learn.microsoft.com/en-us/windows/win32/fileio/programming-considerations-for-transacted-fileio-).
The actual failing case and its passing correction are retained.

Applicable Windows checks passed:

- 30 registration/native library tests. Abrupt termination covers uncommitted
  existing writes and new-file creation with the data handle open or closed.
  Another owned Rust process is killed after publishing the first new file in
  a mixed journal; fresh recovery removes both its destination and remaining
  stages, restores existing bytes and removes owned links without Rust destructors.
- Four new public creation tests and four existing snapshot tests. Creation
  covers Unicode, empty and 128 KiB files, a recently deleted destination,
  repeat/disconnect, existing regular/directory/dangling-link refusal, changed
  bytes, identical-content replacement, journal tampering and size/overlap limits.
- 23 registration integration tests, including interrupted 512-link
  registration; five explicit NTFS 8.3 tests, including new configurations versus
  other configurations, source-link destinations and recovery state, plus a
  successful distinct aliased destination and immediate file/directory-link
  reconnection.
- Core library/tests Clippy and workspace formatting checks.
- Actual model-free Codex CLI 0.153.4 feature preparation, publication and rollback
  in 50.01 s, now including a freshly created config. The native binary is pinned
  in the [feature record](rust-feature-edit.md). Selected source hashes stayed
  unchanged during that run, as did the global base-config hash. No shared service
  or real model request was involved.

```text
cargo test -p harness-core --lib registration --offline --locked --jobs 1 -- --nocapture
cargo test -p harness-core --test config_creation --test config_file --test registration --offline --locked --jobs 1 -- --nocapture
cargo test -p harness-core --test registration aliased_ --offline --locked --jobs 1 -- --ignored --nocapture
cargo clippy -p harness-core --lib --tests --offline --locked --jobs 1 -- -D warnings
```

Evidence root:
`%LOCALAPPDATA%/codex-harness-evidence/config-creation-26428eee3c5d4680afd875d2f1ac8e83/`.
`protection.*` retains the initial tunneling failure; `tunneling-fix.*`,
`protection-fixed.*`, `integration.*`, `aliases.*` and `clippy.*` retain checks.
`native.*` and `native-receipt.json` retain the actual CLI run and its source/hash
boundary. Earlier tests were not retroactively assigned this later source receipt.

An independent Astra senior review reproduced the same tunneling failure in
file-symlink publication: apply, disconnect and immediately apply left an
interrupted journal. Explicit recovery succeeded and source data survived. The
parent reproduced the failure through the public API, then made link publication
retain creation time through a writable guard that still verifies exact target,
type and identity. The common publication helper now preserves creation time for
both object types. Three immediate cycles for each file/directory-link type pass;
all affected library/integration/alias checks and Clippy passed again. The reviewer
did not establish whether this problem existed before the current refactor.
`link-tunneling-before.*` and `link-tunneling-after.*` preserve the failure and fix;
the final combined checks are in `final-*`. The independent reproducer is retained
under `%TEMP%/harness-config-review-cc4023fcc9f54863b5327924b7785e78/`.

The same reviewer independently inspected the corrected source and retained
before/after regression evidence and confirmed the reported P1 closed. It did
not replay the broader tests.

This is process-interruption evidence, not power-loss testing. Full
installation/component selection, old script-layout
metadata migration, PATH, relocation and global native activation remain open;
tasks 4.1–4.3 are not closed by this increment.
