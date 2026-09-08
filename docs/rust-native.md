# Native Rust implementation

The [migration specification](../openspec/changes/migrate-harness-to-rust/proposal.md)
requires Rust for all maintained harness-owned executable code, including tests
and skill helpers. This is a rule for `codex-harness` development; it does not
change the languages of projects using the kit. External Codex, OpenCodex,
Serena, Codebase Memory, Graphify, Nuphus and language servers retain their own
implementations and supported runtimes.

## Current entry points

The migration is in progress. Global installation still uses the existing
script lifecycle. The native commands below prepare/check isolated candidates;
they do not yet replace the global launcher or installer.

Prerequisites: Windows x64, the Rust MSVC toolchain (minimum Rust 1.89), Cargo,
the MSVC C++ linker and Windows SDK. The current host verified Rust/Cargo 1.97.1.
Use the existing toolchain and linker; no package installation runs during Check.
Workspace dependencies are locked in the root `Cargo.lock`. The RTK adapter is
a member of this workspace and has no independent dependency lockfile.

From this checkout:

```powershell
cargo build --workspace --locked --jobs 1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --jobs 1 -- -D warnings
cargo test --workspace --locked --jobs 1 -- --test-threads=1
```

The current 16 GiB host experienced allocation failures and misleading missing
stdlib metadata errors under memory pressure. Keep builds and process-resource
acceptance sequential; `--jobs 1` also bounds the native manager's compiler
concurrency without increasing its 2 GiB Job limit. A failed compilation retains
its log and does not alter the active installation. It is not a successful check.

Explicit preparation of a versioned candidate outside the source tree:

```powershell
cargo run --release --locked -p codex-harness -- build --source . --state "$env:LOCALAPPDATA/codex-harness-native"
```

The result identifies the immutable build directory. Check an explicit candidate
with `codex-harness.exe check --build <directory>`. Optional `--source <checkout>`
checks relocation against the selected source without modifying its registration.
Check is model-free and executes no Cargo, project command or hooks.

Select an already verified immutable candidate inside its owned build state:

```powershell
codex-harness.exe activate-build --state <state-directory> --build <build-directory>
codex-harness.exe recover-build --state <state-directory>
```

These commands manage the local candidate pointer; they do not register global
commands or change services. Selection is serialized with builds. A journal
retains the exact previous pointer, and recovery checks expected file contents
and build identity before restoring it. If explicit repair started from an
already damaged build, recovery finishes the verified replacement instead.
Unexpected edits preserve both target and journal for explicit repair. Completed
journals remain in the owned selection history, including damaged starting-state
evidence. Old binaries are never overwritten or removed.
Ordinary consumers must reject an unfinished selection journal or stale build.
An integrity-verified manager may select a repaired build even if the old binary
was altered; recovery refuses to restore an altered previous build as usable.

Build state has an ownership marker, an exclusive kernel lock and separate
staging/build directories. Every compilation uses a fresh owned temporary target
directory with a short path for MSVC; unchanged candidates reuse their verified
immutable binaries. Reusing Cargo's mtime cache across changed source hashes is
unsafe even if the source manifest uses content hashes. A failed build retains
its Cargo log and preserves accepted builds. The source manifest is checked both
before and after compilation. Changing native source/lock inputs makes Check
stale; documentation-only changes do not require compilation. Missing/altered
manager binaries require explicit Cargo bootstrap. An integrity-verified manager
can prepare repairs even with stale source or an altered dependent binary.

Custom compiler wrappers and ambient Rust build overrides are currently rejected
before creating build state. Global activation, update/recover/disconnect,
ordinary launch and complete migrated acceptance remain implementation work;
these initial commands do not establish delivery of that larger contract.

Compiler dep-info is compared with the captured input inventory before accepting
a candidate. Executable text resources belong under the crate's `src` directory;
compiling an omitted documentation file or an input outside owned source/build
roots fails explicitly. Source subdirectories named `tests` or `examples` are
included. Root documentation and noncompiled Markdown outside `src` do not
require recompilation. Ancestor/user Cargo config contents contribute hashes
without being copied into receipts; unsupported ambient compiler override
families are rejected before build reuse or state creation.

The native launcher argument policy is implemented and tested in
`harness_core::launcher`. The actual global launcher still uses its existing
script. Its port must preserve upstream-managed background-process lifetime and
package-manager metadata as well as argv, streams and cancellation; the bounded
job's whole-tree cleanup cannot be applied to the upstream CLI without checking
that behavior. The adopted npm entry point in Codex 0.153.4 forwards arguments
and signals and supplies `CODEX_MANAGED_PACKAGE_ROOT` and its manager selector.
Native profile/config precedence is documented in
[OpenAI's configuration contract](https://learn.chatgpt.com/docs/config-file/config-basic#configuration-precedence).

## Compatibility during concurrent changes

Before native lifecycle integration, the current `linked-global-kit` source-update
requirement was compared with both pending deltas. The main still has the three
original data-file/registration/relocation scenarios. The migration adds explicit
compiled freshness and integrity-verified manager recovery;
`autonomous-skill-evolution` adds current-session skill revision awareness and
scoped registration. These are complementary obligations: executable helpers
follow build integrity, source-owned skill descriptors/resources remain live, and
the accepted skill revision must be delivered/recovered in an existing session
without requiring its entire initial prompt to reload. At each sync, merge the
then-current main and preserve **all** scenarios from both deltas; do not replace
one complete requirement body with the other. Neither workflow's unchecked tasks
are completed by recording this compatibility rule.

The current resource selection must survive every native port: explicit indexing
only, one account-wide indexing admission slot, a 2048 MiB/25% CPU/600-second
CBM boundary, shared project-isolated Serena workers, and ownership-aware cleanup.
The late explicit Rust selection in Serena joins retained Python; it does not
restore automatic diagnostics. `global/hooks.json` stays empty and the separate
RTK definition remains the accepted `PreToolUse` explicit-command exception.

The original RTK script lifecycle has been adjusted to read the root workspace
manifest/lock and build only `harness-rtk`. This is a transitional compatibility
edit; native RTK lifecycle and Rust acceptance-helper migration are still needed.
The global RTK selection was rebuilt only after its applicable existing native
adapter checks and isolated lifecycle checks passed, then checked outside the
checkout. No subscription service or unrelated capability was reconfigured.
