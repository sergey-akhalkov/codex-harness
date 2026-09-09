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

Windows also keeps a running test executable locked after Cargo releases its
build lock. During parallel source work, a later compilation into that same
target directory produced LNK1104. Keep build/resource checks sequential and use
an explicit separate `--target-dir <owned-verification-directory>` when a retained
test executable could still occupy the default target. This is a Cargo test
output choice, not an ambient `CARGO_TARGET_DIR` override for native build identity.
The isolated retry passed; the original failure log remains in the PATH evidence
linked below.

Explicit preparation of a versioned candidate outside the source tree:

```powershell
cargo run --release --locked -p codex-harness -- build --source . --state "$env:LOCALAPPDATA/codex-harness-native"
```

The result identifies the immutable build directory. Check an explicit candidate
with `codex-harness.exe check --build <directory>`. Optional `--source <checkout>`
checks relocation against the selected source without modifying its registration.
Check is model-free and executes no Cargo, project command or hooks.

`codex-harness.exe inventory --source <checkout> --codex-home <directory>
--user-home <directory>` reads the live native manifest and reports core data
links/names without mutation. Its [current scope](evidence/rust-inventory.md)
does not yet include the full installer or dependency checks.

Native inventory holds the [legacy-compatible installation mutex](evidence/rust-installation-lock.md).
`codex-harness.exe inspect-installation --codex-home <directory> --user-home
<directory> [--dependency-user-home <directory>]` reads and validates the old
installation metadata without changing it; [metadata acceptance](evidence/rust-installation-state.md)
distinguishes owned/adopted links and unresolved migration work.

`codex-harness.exe outcome-report --input <private-json> [--markdown]` formats
local attempt accounting without executing anything. [Native report acceptance](evidence/rust-outcome-report.md)
records retry/verification accounting, comparison exclusions, input bounds and
actual outside-checkout CLI checks.

`codex-harness.exe delegation-usage [ROLLOUT ...] [--format json|markdown]
[--output PATH] [--private-sources PATH]` reads explicit local telemetry without
model calls. [Native usage acceptance](evidence/rust-delegation-usage.md) records
the ported cases, attribution/privacy boundaries and a real-log comparison.
Outcome execution/oracle migration and global lifecycle integration remain open.

`codex-harness.exe outcome-run --request PATH --run-model-probes` executes one
explicit native launcher in an isolated temporary case/home. Its [request,
evidence and process acceptance](evidence/rust-outcome-run.md) keep execution
status separate from correctness; the command defaults to a model-free skip.
Comparison preparation and independent outcome oracles still require migration.

`codex-harness.exe outcome-discover --request PATH` performs model-free native
skill/config discovery in an isolated temporary case/home. Its [protocol and
acceptance](evidence/rust-outcome-discovery.md) cover input identity, bounded
process lifetime and incomplete/contradictory responses. Detailed JSON is private.

`codex-harness.exe outcome-arm --request PATH` prepares and verifies both skill
comparison modes in an owned temporary installation. [Arm acceptance](evidence/rust-outcome-arm.md)
covers schema 1/2 link checks, all discovered registrations, exact config rollback
and installed native discovery. Full suite/oracle and lifecycle migration remain open.

[User PATH publication](evidence/rust-environment-path.md) provides exact-value
TxR publication/rollback and atomic file receipts with owned-key and killed-process
acceptance. Native installer journal orchestration remains unfinished; the real
user PATH has not been changed by this acceptance.

The [native registration foundation](evidence/rust-registration.md) adds
journaled source links with object identity, protected publication/deletion and
actual interrupted-process recovery. It currently requires local NTFS with TxF
and is not yet connected to the global installer.

[Existing-link changes](evidence/rust-link-changes.md) preserve original objects
for rollback during replacement/removal. The [finish protocol](evidence/rust-registration-finish.md)
commits a journaled metadata witness and safely retires rollback objects. The
current [schema 8 builder](evidence/rust-registration-metadata.md) supplies actual
object IDs to [stable installation metadata](evidence/rust-installation-metadata.md).
Full lifecycle integration remains unfinished.

[Native feature preparation](evidence/rust-feature-edit.md) uses the original
Codex executable to edit and observe a candidate in an owned home.
[Existing-file publication](evidence/rust-config-file.md) checks original bytes
and object identity, with atomic commit and guarded rollback. The registration
journal includes these existing-file changes and
[exclusive creation of absent configurations](evidence/rust-config-creation.md).
Full installer orchestration remains unfinished.

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
The [version handoff contract and acceptance](evidence/rust-self-update.md) explain
fresh-manager finalization, explicit candidate Check/activation, and the required
Cargo bootstrap for preprotocol managers. Ordinary consumers must reject an
unfinished selection journal or stale build.
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
source-owned skill/configuration data, including the inspection schema, remains
live filesystem input and must not be embedded in a deployed binary. Therefore
compiling an omitted documentation file or an input outside owned source/build
roots fails explicitly. Source subdirectories named `tests` or `examples` are
included. Root documentation and noncompiled Markdown outside `src` do not
require recompilation. Ancestor/user Cargo config contents contribute hashes
without being copied into receipts; unsupported ambient compiler override
families are rejected before build reuse or state creation.

The native launcher and argument policy are implemented and tested in
`harness_core::{native_launcher,launcher}`. An owned registered release candidate
passed [actual CLI and console acceptance](evidence/rust-launcher.md), preserving
upstream-managed background-process lifetime, package-manager metadata, argv,
streams and Ctrl+C. The actual global launcher still uses its existing script
until journaled global installation is ready. The adopted npm entry point in
Codex 0.153.4 supplies `CODEX_MANAGED_PACKAGE_ROOT` and its manager selector.
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
