# Native Rust implementation

The [migration specification](../openspec/changes/migrate-harness-to-rust/proposal.md)
requires Rust for all maintained harness-owned executable code, including tests
and skill helpers. This is a rule for `codex-harness` development; it does not
change the languages of projects using the kit. External Codex, OpenCodex,
Serena, Codebase Memory, Graphify, Nuphus and language servers retain their own
implementations and supported runtimes.

## Current entry points

The explicit native core `install` / `update` / `check` / `recover` / `disconnect --core-only` increment and its
remaining lifecycle work are recorded in [core installation acceptance](evidence/rust-core-installation.md).

`codex-harness.exe diagnose` and the native core installation's
`codex-harness-check.exe` alias provide read-only source reports. The port,
process/privacy boundaries and checks are recorded in
[native source diagnostics](evidence/rust-source-diagnostics.md). Global cutover
has not yet replaced the current script diagnostic alias.

The native `dependencies discover --source CHECKOUT` and explicit
`dependencies plan --source CHECKOUT` commands and their
read-only/package-version boundaries are recorded in
[dependency observation evidence](evidence/rust-dependency-discovery.md).
`dependencies audit --package-root DIRECTORY` compares selected npm package
files with their exact official archive in a bounded worker;
[archive/audit evidence](evidence/rust-dependency-archives.md) records its scope
and the remaining provisioning/transaction work. Explicit
`dependencies stage --package NAME --version VERSION --state DIRECTORY` prepares
an owned candidate without activation. `dependencies probe --executable FILE
--kind codebase-memory|nuphus --sha256 DIGEST` checks an explicitly selected,
digest-pinned native MCP: Codebase Memory uses an owned inert source sample;
Nuphus lists its protocol/tool contract without desktop or browser actions.
The caller must establish the executable and companion DLL provenance first.

`dependencies validate`, `select`, `selected`, `recover-selection` and
`rollback-selection` connect retained candidates to explicit runtime validation
and journaled local selection. Run each command with `--help` for required
stage/digest/state/slot options. BasedPyright requires an explicit Node path and
digest. [Selection acceptance](evidence/rust-dependency-selection.md) records
actual package runs, conflict/rollback checks and the remaining global connection.

`dependencies resource-check --cache DIRECTORY` reads persisted CBM policy.
`dependencies cbm-index --help` describes explicit audited indexing into a
selected cache. [Native CBM evidence](evidence/rust-cbm-runtime.md) records the
current checks, compatibility limits and unfinished adapter acceptance.
`dependencies cbm-catalogue --help` describes isolated catalogue retrieval under
account admission. [Native stdio evidence](evidence/rust-mcp-stdio.md) separates
the protocol/session increment from the remaining installed proxy migration.
`dependencies cbm-tool --help` describes explicit non-index tool calls on a
selected cache. A cache held by another daemon is refused before launch;
shared broker clients remain unfinished. Missing CBM UI JSON is treated as
enabled for the audited embedded-UI binary, regardless of SQLite settings.
`mcp codebase-memory --help` connects the explicit CBM paths and a saved
catalogue report to a bounded native stdio connection. Its handshake is local;
tool calls verify the executable and resource policy. See the
[stdio integration record](evidence/rust-mcp-stdio.md).

`dependencies stage-python --help` describes creation of an empty offline UV
environment using explicit trusted executable hashes. [Python staging evidence](evidence/rust-python-staging.md)
separates this unactivated candidate from full runtime and package provisioning.

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
Suite/oracle migration and global lifecycle integration remain open.

`codex-harness.exe outcome-run --request PATH --run-model-probes` executes one
explicit native launcher in an isolated temporary case/home. Its [request,
evidence and process acceptance](evidence/rust-outcome-run.md) keep execution
status separate from correctness; the command defaults to a model-free skip.
Full suite preparation and independent outcome oracles still require migration.

`codex-harness.exe outcome-prepare --case <controlled-case> [--observer <exe>]`
creates fresh owned temporary inputs and instructions without models or builds.
[Controlled case acceptance](evidence/rust-outcome-cases.md) records the five
supported cases, copied executable identities and native consumer checks.

`codex-harness.exe outcome-oracle --request <private-json>` checks those five
cases using host-frozen preparation and native execution evidence.
[Oracle acceptance](evidence/rust-outcome-oracle.md) records the actual targets,
failure cases, evidence bounds and unfinished external/suite integration.

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

Native core `install`/`update --core-only` default to User PATH. Optional
`--path-scope User|Process` selects a new connection's scope; an existing
installation keeps its recorded scope, and an explicit differing scope requires
disconnection first. Process PATH belongs to the current manager command and its
descendants and cannot change the calling PowerShell environment. Owner identity
is PID plus creation time: same-owner exact before/after restore, live foreign
owner refused, dead or reused owner a read-only no-op. Mutation uses the Process
PATH module mutex, not an arbitrary external PATH CAS. Journal schema 11 carries
mutually exclusive `path_change`/`process_path` actions and remains
read-compatible with schema 8/9/10, including schema-10 metadata retirement.
The User PATH receipt format is unchanged. Check, recover and disconnect derive that
recorded scope. Evidence and remaining lifecycle work are in
[core installation acceptance](evidence/rust-core-installation.md).

Native `recover --core-only --preview` reports rollback, finish or no pending
operation after inspecting the selected core journal and owner state. It opens
only existing registration files, creates no target homes or recovery files,
and makes no PATH changes. An actual recovery rechecks the state before writing;
preview is not a commitment to the observed plan. Combined component recovery
remains separate unfinished work.

Current native PATH receipts retire atomically with their terminating recovery
record after a successful rollback or committed finish. Historical files without
a live intent/commit are preserved. The
[cleanup acceptance](evidence/rust-core-installation.md#atomic-retirement-of-current-path-receipts)
includes owned registry checks, interrupted processes and foreign-receipt refusal.

The [native registration foundation](evidence/rust-registration.md) adds
journaled source links with object identity, protected publication/deletion and
actual interrupted-process recovery. It currently requires local NTFS with TxF
and is not yet connected to the global installer.

[Existing-link changes](evidence/rust-link-changes.md) preserve original objects
for rollback during replacement/removal. The [finish protocol](evidence/rust-registration-finish.md)
commits a journaled metadata witness and safely retires rollback objects. The
builder [introduced in schema 8](evidence/rust-registration-metadata.md) supplies actual
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
before creating build state. Core connection and schema-2 recovery/disconnection
now have [native acceptance](evidence/rust-core-installation.md); complete component
orchestration, global activation and the full migrated acceptance remain unfinished.
Standalone PowerShell `pending.json` also has a native rollback consumer, including
an actual old-installer/new-manager recovery case. Combined component journals and
recovery preview remain separate unfinished work in that acceptance record.

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
