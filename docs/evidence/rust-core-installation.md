# Native core installation increment

This completes the manifest/registration foundation in [migration task 4.1](../../openspec/changes/migrate-harness-to-rust/tasks.md). Tasks 4.2–4.4 still require their remaining lifecycle/command acceptance; global cutover is separate.

The Rust manager exposes `install` / `update --core-only`, requiring absolute
`--source`, `--build`, `--codex-home` and `--user-home` inputs. Optional
`--upstream` selects an absolute native executable, supported Codex package root
or entrypoint. Without it, an update keeps its recorded native path; a fresh
installation searches absolute PATH entries, excluding its harness commands.
`--dependency-user-home` defaults to the connection owner; `--preview` reads
ownership, build/configuration data and PATH without executing commands or
creating target homes. Both owner mutexes cover actual connection work.
Optional `--path-scope User|Process` selects PATH publication for a new
connection; User remains the default. An existing installation keeps its recorded
scope, and an explicit differing scope requires disconnection first.

The manager also exposes `recover --core-only` and `disconnect --core-only`
with the three owner-home options. Both support `--preview`; disconnection currently
supports legacy schema-1 and native schema-2 installation metadata. Check,
recover and disconnect use that recorded PATH scope. Disconnect removes
only recorded owned link objects, tolerates already missing or dangling owned
links, and preserves adopted resources, including later user edits. Neither
source availability nor a working upstream CLI is required for disconnection.
For legacy disconnection, the journal retains the original metadata bytes for
rollback and publishes a minimal native owner witness for committed recovery.
This imports no adopted deletion authority and requires no intermediate install.

`check --core-only` verifies current native metadata, all recorded link IDs,
the five executable registrations, the native diagnostic alias, immutable build/upstream identity and every
data link in the current manifest. It runs the model-free runtime observation,
then rechecks source, metadata, launch registration and links. User PATH presence
is a registry observation, not proof of fresh-terminal or PowerShell precedence.
Missing, legacy, pending or stale state is refused without repair or provisioning.
Process PATH presence is an observation of the current manager command and its
descendants; it cannot change the calling PowerShell environment.

`core_install::connect` imports legacy/schema-2 metadata, verifies live link IDs,
preserves adopted links, and retains recorded hooks during core updates. Fresh
core installation adds no hooks. The original CLI's feature editor disables
ordinary hooks unless the recorded RTK selection is enabled, preserving manual
suspension in that case. Configuration publication shares the link journal.

Launcher registration schema 2 names an immutable build directly. Its generated
machine-local JSON is linked at `harness/native-launch.json` alongside the native
executable links. A later build-tool selection cannot silently move this runtime.
Schema-1 launcher registrations remain readable.

The same journal now registers `harness/bin/codex-harness-check.exe` against the
selected manager. Its source observations and acceptance are described in
[native source diagnostics](rust-source-diagnostics.md).

Registration schema 10 includes the exact PATH change and, for disconnection,
the existing metadata file to retire after the irreversible commit decision.
The same metadata ID and bytes remain the recovery witness until that decision;
cleanup never adopts a replacement file. The registry write and
receipt share a Windows transaction. The receipt and its filename bind the
journal's path, ID and bytes, distinguishing identical successive plans. Runtime
acceptance precedes completion: interruption before completion rolls back;
interruption after completion can finish the accepted candidate. Schema-8 intent
without PATH and schema-9 intent remain recoverable; older schemas cannot carry
the newer PATH or metadata-retirement actions.
Schema 11 journals mutually exclusive `path_change`/`process_path` actions and
remains read-compatible with schema 8/9/10, including schema-10 metadata
retirement. The User PATH receipt format is unchanged. Process PATH binds owner PID plus
creation time: the same owner restores only an exact before/after value; a live
foreign owner is refused; a dead or reused owner is a read-only no-op. Mutation
uses this module's mutex, not an arbitrary external PATH CAS.

Recovery verifies both installation owners against the exact metadata bytes and
ID embedded in the journal or commitment, including after the live metadata and
original journal have been retired. The installed manager resolves its command
symlink before verifying its own build identity. An intact manager can repair
stale sources or altered dependent binaries; an altered manager is refused.

`core_runtime::verify` uses model-free original CLI `--version` / `--help` in a
private home, then executes the installed native launcher symlink with
`debug prompt-input` from the target's `harness` directory. It checks the complete
selected instructions and Full Access permission markers. Windows Jobs bound
execution; process outputs and original errors remain private. This check covers
instruction/profile loading, not every skill, MCP or subscription consumer.

## Evidence

Machine-local evidence:
`%LOCALAPPDATA%/codex-harness-evidence/core-lifecycle-29d511c364fb499286ae57b389e34c6a`.

- `launcher.stdout`: six actual launcher cases passed, including immutable
  selection, ambiguous binding refusal and stale source rejection.
- `activation-killed.stdout`: five activation tests passed, including actual
  killed Rust children during validation and after completion. Registry writes
  used owned test keys, never the user's real PATH.
- `core-suite.stdout`: 124 core tests passed before consumer modules were wired;
  eleven explicit fixture/platform cases were ignored.
- `core-components.stdout`: preview plus three runtime helper tests passed.
- `real-core.stdout`: original CLI 0.153.4 and a compiled Rust launcher completed
  three connections; malformed live profile rejection preserved exact prior
  metadata and link IDs. Duration: 135.80 seconds, no model calls.
- `real-core-2.stdout`: two expanded real-CLI cases passed in 209.26 seconds:
  legacy import, adopted profile preservation, inherited hook repair and foreign
  replacement refusal; repeated connection, runtime failure recovery and source
  relocation. Homes and registry keys were owned fixtures.

Independent review found a P1 window between releasing the receipt's journal
guard and capturing the finish commitment. The correction keeps the same held
journal through both operations. `review-fix.stdout` confirms the dedicated
replacement attempt is blocked while committed metadata remains present.
`clippy-final.stderr` records clean Clippy for both packages and all targets.

Subsequent recovery and disconnection evidence:

- `recovery-owners.stdout` and `recovery-cli.stdout`: both-owner refusal before
  undo/committed cleanup, including actual native `recover` execution.
- `manager-integrity-3.stdout`: the actual installed manager link permits
  recovery with stale source or an altered dependent artifact and refuses an
  unrecorded manager name or modified manager executable. An earlier test had
  incorrectly expected dependency damage to disable the intact repair manager;
  its failure is retained in `manager-integrity.stderr`.
- `disconnect-tests.stdout`: four tests passed, covering missing/dangling owned
  links, adopted edits, exact foreign-metadata refusal, and five injected
  interruption phases. Source trees remain intact.
- `retirement-core-suite.stdout`: 133 core tests passed, fourteen explicit cases
  ignored, before the subsequent dedicated process/PATH tests were added.
- `disconnect-cli.stdout`: actual native command verified owner refusal,
  preview preservation, disconnect and repeated no-op. This fixture owned no
  PATH entry, so the CLI performed no registry write.
- `disconnect-killed.stdout`: five Rust children were forcibly terminated at
  pre-acceptance, completion, commit, metadata-retirement and journal-retirement
  boundaries; owner recovery restored or finished the durable phase.
- `disconnect-path.stdout`: the combined PATH/retirement test passed on owned
  registry keys, including incomplete rollback, accepted cleanup and a foreign
  PATH conflict that preserves pending state.
- `schema-compatibility.stdout`: schema-8/9 recovery and finish passed; attempts
  to carry actions introduced by newer schemas were refused.
- `disconnect-clippy.stderr`: clean Clippy for both packages and all targets
  after these changes.
- `retirement-integration-2.stdout`: thirty registration/link/finish integration
  tests passed, eight explicit child/platform cases ignored. The earlier command
  nominated the non-existent `registration_metadata` integration target; that
  module is covered by the library suite, and Cargo's error is retained.
- `retirement-cli-integration.stdout`: seven actual launcher tests passed.
- `core-check-final.stdout`: eight focused Check tests passed. Parent integration
  corrected the worker's missing `Debug` test bound and repeated fresh-fixture
  publication, then added current-manifest link coverage.
- `real-check-disconnect.stdout`: an original CLI 0.153.4 connection, successful
  native Check, newly unregistered skill refusal and native disconnection passed
  in 64.55 seconds. Config/metadata remained unchanged during Check; disconnect
  restored the owned registry test key's initial PATH and retained source files.
- `check-cli.stdout`: actual manager Check refused missing/legacy/pending state
  without rewriting it. The positive Check used the Rust library entry point
  with an owned registry key and the actual installed launcher; positive manager
  CLI acceptance against the real user PATH remains part of global delivery.
- `check-final-clippy-2.stderr`: clean Clippy for both packages and all targets
  after Check integration and a corrected test-only import.
- `legacy-disconnect-tests.stdout`: eight disconnection tests passed, three
  explicit cases ignored, after adding legacy removal. This includes absent and
  dangling old links, adopted edits and foreign-owned refusal; five legacy
  interruption phases restore the exact schema-1 bytes or finish owner-bound
  cleanup. The existing native killed-process cases also passed.
- `legacy-disconnect-cli.stdout`: the rebuilt manager passed preview, wrong-owner
  refusal, disconnect and repeat for both schema-1 and schema-2 installations.
  Fixtures owned no PATH entry; old launcher targets were inert, never executed.
- `legacy-disconnect-clippy.stderr` and `legacy-final-fmt.stderr`: clean Clippy
  for both packages/all targets and a passing workspace format check.

A separate source review of retirement and owner recovery found no concrete
P0/P1. Its scope excluded global activation and runtime consumers. Real-CLI
connection fixtures bind minimal owned source trees and inert unused executable
placeholders alongside the compiled launcher; they do not certify a complete
production build or global installation.

Process PATH/schema 11 evidence in the same directory:

- `process-path-owner.stdout`: three owner tests passed, one owned child
  ignored. Same-owner exact restore, live foreign owner refused, dead/reused
  owner no-op.
- `process-schema-compatibility.stdout`: seven tests passed, including schema
  8/9/10 recover/finish and schema-10 metadata retirement; older schemas cannot
  carry process PATH.
- `process-real-cli.stdout`: one explicit original-CLI/manager case passed in
  109.58 seconds at `%TEMP%/native-core проба-u6dvjH`. Preview, connect, repeat,
  CLI check and CLI disconnect used Process scope with no registry writes; the
  parent PATH was unchanged.
- `process-disconnection.stdout`: two tests passed in 4.70 seconds. Same-process
  metadata/link/PATH inverse across seven phases, including core disconnect
  preview/success, incomplete rollback, committed retirement and a foreign PATH
  preserve/retry; five killed Rust children left the recovering parent PATH
  untouched.
- `process-core-suite.stdout`: 150 library tests passed, twenty explicit cases
  ignored, in 63.07 seconds.
- `process-clippy.stderr`: clean Clippy for both packages and all targets after
  these Process PATH checks.

## Complete checkout core consumer

`full-candidate-build.stdout` records a fresh native manager build from the
then-current checkout, offline with one Cargo job and a 2048-MiB compiler Job limit.
All five release artifacts bind source identity
`38c890ac62eaefb954dc546e95c6dac8e597ee7c63ba4a293acee2188bf5a05c`.
The immutable build is under the evidence directory's
`full-candidate-state/builds/38c890ac62eaefb9-1788972978853497800-9448`.
That identity remains historical task 4.1 acceptance evidence. Process PATH
source was added after it, so the candidate is now source-stale and is not a
current healthy or global claim.

`full-core.stdout` records an explicit owned full-checkout core consumer passing
in 83.73 seconds: preview created no target home; connection linked all 15
current data entries, five native commands and launch registration; Check loaded
the actual instructions/profile and preserved metadata; update changed no links;
disconnect removed all 21 owned links and restored the test key's initial PATH.
`%TEMP%/native-full-core-dMmljv/full-core-result.json` retains the three private
runtime receipts and source/build references. This uses the complete release
build and real current data, with no synthetic executable placeholders. It
exercises core delivery on owned targets, not MCP/subscription component delivery
or global activation.

Task 4.1 also reuses the focused [inventory](rust-inventory.md),
[registration](rust-registration.md), [configuration](rust-config-file.md),
[created configuration](rust-config-creation.md) and
[paired ownership lock](rust-installation-lock.md) evidence. Those checks cover
machine-local destination resolution, exact ID/byte conflicts, interrupted
publication, foreign-object preservation and two-owner serialization. Source
data remains linked and credentials are not put into reusable artifacts.

## Legacy pending journal rollback

`recover --core-only` now consumes the old PowerShell `harness/pending.json`
under both installation-owner locks. The old format has no schema number or
commit decision: even a successful `-DeferCommit` installation is rolled back.
The parser validates the recorded owners, allowed destinations, source claims,
scope and bounds; unrelated fields and skill names differing from folders remain
supported. Exact `stateBeforeBytes` take precedence. Older records without those
bytes restore the recorded previous JSON; records without `stateAfterHash`
accept only absence, known restore bytes or matching recorded JSON. Empty files
and absent files remain distinct when that distinction is recorded.

Recovery holds the immutable journal and captured live objects. Uncommitted
temporary children hold the parent directories through deletion and recreation,
then disappear on release. Link types are checked against their allowed
destinations, including newly added links with no old target. Inverses tolerate
already restored or absent objects and dangling source targets. Conflicting
native or combined-component journals are preserved for their own lifecycle.

Legacy PATH records contain only strings. User recovery compares expanded text
for REG_EXPAND_SZ and writes the recorded string as REG_SZ, matching the old
[.NET environment implementation](https://github.com/dotnet/runtime/blob/main/src/libraries/System.Private.CoreLib/src/System/Environment.Windows.cs)
and [registry expansion contract](https://learn.microsoft.com/en-us/dotnet/api/microsoft.win32.registrykey.getvalue?view=net-10.0).
Already restored registry bytes are retained. Process recovery requires a
matching current command value; the old journal has no process identity. These
records cannot prove historical object IDs or reconstruct unrecorded registry
representation. Configuration edits and empty directories absent from the old
inverse history are not retroactively rolled back.

The old PowerShell `ValidateSet` accepts and preserves ASCII case variants such
as `pRoCeSs`. Native metadata and pending-journal readers now accept that spelling
and serialize new records canonically as `User`/`Process`. A direct installed
PowerShell probe confirmed mixed-case preservation and refusal of whitespace
and long-s lookalikes. `legacy-scope-case-baseline.stdout` records the prior
actual CLI rejection; `legacy-scope-case-fixed.stdout` records three passing
CLI tests, including exact original metadata preservation. Unknown scopes and
non-string values remain invalid.

An independent source review found two P1 races: ordinary-parent replacement
and a wrong-type new link with the expected target. The parent leases and type
checks address them; their owned counterexamples passed in
`legacy-pending-second.stdout`, together with five actually killed recovery
processes, both PATH scopes, foreign edits and source relocation. Sixteen tests
passed; two explicit cases were ignored. `legacy-namespace-lease.stdout` also
verifies that both staged files and links prevent parent rename/reparse mutation
and disappear on abort. The first combined run's hashless-metadata failure is
retained in `legacy-pending-first.stderr`; the corrected run preserves that
foreign metadata.

`legacy-producer-2.stdout` records the actual old installer with the full checkout,
original CLI 0.153.4 and `-DeferCommit` in an owned Process-scope fixture outside
the checkout. The rebuilt native manager then removed its 17 links, metadata
and journal, reporting `recovered-legacy`, `committed: false`, and no model calls.
The caller's Process PATH and User PATH were unchanged; source targets remained
present, as recorded in `legacy-produced-native-recover-result.json`. This actual writer
case carries a state hash; the separate Rust fixtures cover historical shapes.

After the final hashless-state correction, `legacy-pending-core-suite.stdout`
records 169 passing library tests and 22 explicit ignored cases.
`legacy-pending-clippy-2.stderr` and `legacy-final-fmt.stderr` are clean.
`legacy-native-cli.stdout` verifies recovery and repeat through the final rebuilt
manager in `%TEMP%/native-legacy-recover-6lqeyj`, preserving the caller and User
PATH. The source hashes, executable hash and exact commands for this checkpoint
are retained in `legacy-recovery-acceptance.json` in the same evidence directory.

## Recovery preview

`recover --core-only --preview` returns `status: "preview"`, `action: "rollback"`,
`"finish"` or `"none"`, the selected `journal` (`"native"`, `"legacy"` or null),
and `model_calls: 0`. It does not report an operation as committed. Native
completion/commit records select finish; incomplete native and old PowerShell
core journals select rollback. Real recovery repeats its ownership checks.

Preview uses both installation mutexes and opens an existing native lock file
without creating or writing it. Missing homes stay absent; unowned state or a
missing/busy native lock is preserved and refused. Exact owner, journal,
completion/commitment, metadata and available link inverse checks precede a
plan. User PATH receipts are read without a registry transaction; both already
applied and already undone receipts retain their original representation.
Process PATH uses the same current/live/dead owner rules as actual recovery.
Legacy preview shares the current-state checks but returns before staging
metadata or acquiring the temporary child leases used by real rollback.

This is an observation, not a durable reservation of the plan or proof that a
later write will succeed. It does not compile, launch Codex, provision, or write
PATH, links, configuration, metadata, receipt or recovery files.

`recovery-preview-first.stdout` verifies the native fresh-install recovery
matrix and both owner refusals. `recovery-preview-native.stdout` records two
tests covering absent homes, incomplete ownership, both lock layers, and User
PATH before/after rollback, completion and commitment, plus foreign PATH.
`recovery-preview-disconnection.stdout` records five passing cases (one explicit
fixture ignored), including ten actually killed disconnection children across
User and Process metadata and repeated preview before recovery. Snapshots compare
namespace, regular bytes, file/link identities and targets without following
source links; Process and User PATH observations stay unchanged.

`recovery-preview-native-cli.stdout` exercises the rebuilt manager outside the
checkout on incomplete/completed native installations: wrong owner with/without
preview, valid preview, then actual recovery. One test passed in 1.22 seconds;
no preview creates a completion decision or changes the existing metadata.
The initial focused Clippy check is in `recovery-preview-clippy.stderr`.
`recovery-preview-core-suite.stdout` records 172 passing library tests and 22
explicit ignored cases before the additional legacy-preview tests.
`recovery-preview-legacy-first.stdout` records two passing tests for both legacy
PATH scopes, repeated preview/recover and ten foreign-state/owner conflicts.
`recovery-preview-legacy-cli.stdout` records a passing actual manager test in
0.33 seconds outside the checkout: two previews preserve the fixture, real
recovery restores it, and a final preview reports no pending operation. Its
logs are outside the snapshotted fixture in `%TEMP%/native-legacy-recover-cli-Dvueqc`.

## Explicit retirement of absent obsolete links

The core update plan nominates an obsolete owned destination explicitly when it
is already absent. The metadata builder consumes a nomination only for an owned
prior record omitted from the desired set, rechecking ordinary parents and
absence. Duplicate, unrelated, still-desired or adopted nominations are refused.
The default component builder still refuses silent omission of an owned record.
Retirement only removes the metadata record; it never deletes an object at that
name. A foreign arrival before the builder's check is preserved and stops it.

`absent-retirement-first.stdout` records two passing tests across native/legacy
metadata, successful completion and interrupted rollback to original history,
plus foreign file/link arrival and invalid nominations. Source bytes stay intact.
`absent-retirement-real-cli.stdout` records the actual original Codex CLI and
rebuilt Rust launcher/manager in `%TEMP%/native-core проба-Y1Drat`. After initial
connection and repeat, the source manifest stops selecting a skill whose
destination was already removed. Preview preserves original metadata; update
forgets that obsolete record, keeps its source file, and passes subsequent
native Check/disconnect without changing User PATH or the caller's Process
PATH. This one test passed in 147.63 seconds on the owned miniature source
fixture; it is not the full-checkout candidate or global activation.

The combined final checkpoint is `preview-retirement-core-suite.stdout`:
176 library tests passed, 23 explicit cases were ignored, in 75.07 seconds.
`preview-retirement-clippy.stderr` is clean for both packages/all targets;
workspace formatting, strict change validation and all 36 local documentation
links passed. Explicit Serena diagnostics for the consumer, metadata builder
and legacy preview tests were empty. `preview-retirement-acceptance.json`
records source/binary identities, commands and the separate actual CLI checks.
The initial formatting mismatch from the worker's edition-2021 standalone
formatter is retained in `preview-retirement-fmt.stdout`; workspace formatting
uses the project's edition-2024 settings. No global activation occurred.

## Atomic retirement of current PATH receipts

Successful native rollback now retires its exact applied/undone receipts and
journal/completion together. Committed finish retains its durable decision
during progressive backup/journal cleanup, then retires that commit and any
remaining applied receipt atomically. A replaced receipt is refused and preserved.
The verified commitment's embedded intent supplies receipt scope after the live
journal is gone. This changes no journal or receipt schema and never replays PATH
publication during committed cleanup.

The existing owner guard holds the ordinary parent while regular-file guards
transfer to one NTFS transaction. Original parent handles stay held; each ID
and byte hash is checked again before staging deletion. No source link or file
in another parent can enter the batch, and the owner stays intact. A transaction
failure or process termination before commit leaves every file in that batch intact.
Historical files with no live intent/commit are not swept by this operation.

`atomic-cleanup-first.stderr` preserves the initial internal-volume-path reopen
failure. The corrected `atomic-cleanup-second.stdout` has four passing tests and
one explicit child fixture ignored: all failure boundaries, same-byte new-ID and
changed-byte/later-file conflicts, retained-parent checks, and three actually
killed cleanup children before/after commit. `atomic-cleanup-user-completion.stdout`
exercises six User PATH phases through the actual core API against an owned
registry key, including receipt absence after rollback/finish and refusal/retry
of a replaced committed receipt.

`atomic-cleanup-integration.stdout` records 180 passing library tests and 38
passing configuration/link/registration integration tests; 32 explicit cases
were ignored across these suites. Clippy for both packages/all targets and
workspace formatting passed. The rebuilt manager's outside-checkout
preview/recovery case passed in 1.47 seconds (`atomic-cleanup-native-cli.stdout`);
that CLI fixture owns no User PATH entry. An independent source review found no
P0/P1 issue in the inspected cleanup/receipt/recovery paths and test cases; it
did not independently execute tests. Explicit Serena diagnostics were empty.
`atomic-cleanup-acceptance.json` records this checkpoint and its limits.

## Management-time upstream discovery

`native_upstream::resolve` reads an explicit native executable, a Codex package
root or `bin/codex.js`, or the inspected npm `codex.ps1`/`codex.cmd` shim format.
It never executes a shim. The recognized shim fingerprints come from the
installed official CLI 0.153.4; an unknown/custom shim requires explicit selection
of its package or native executable. Package lookup covers the Windows x64
nested/hoisted platform package and the vendor fallback. A resolved platform
package with a conflicting version or missing binary is an error, without
falling back to another executable.

Fresh-install PATH search ignores empty/relative entries, follows directory
order, and refuses distinct eligible commands in the same directory. Current
build binaries and installed harness aliases are excluded by canonical path
and content hash, including copied launchers. A dangling excluded alias keeps
its name exclusion. Metadata/hints are bounded to 64 KiB, ancestry to 64 levels
and PATH to 256 entries. These are bounded Windows package rules, not a general
interpreter for arbitrary package-manager scripts or Node resolution extensions.

Package-manager selection first checks package-bound Vite+ and pnpm ownership,
then the official Bun environment/path hints, with npm as the fallback. The
package's build-time `packageManager` field is not installation ownership.
The resulting native executable/hash, package root/manifest hash and manager
are recorded in the existing launcher schema; ordinary launch performs no
discovery. Install/update use that same executable for feature editing and
runtime acceptance. An omitted `--upstream` on update uses the saved native
path, so the original command need not remain in PATH.

The original package, JS entrypoint, both installed npm shims and direct native
binary resolved identically in `upstream-actual-package.stdout` (one passing
explicit read-only test, 25.20 seconds). This used the installed official
`bin/codex.js` and package metadata as the current contract. The resolved binary
SHA-256 was `444a3f0008050605cae73cd9b7a2dcac61294062dfaab56dd20430fd6498518b`.
This test launches no subprocess or model and modifies no package files.

`upstream-real-cli-final.stdout` records the passing actual manager lifecycle
case in 100.34 seconds, under `%TEMP%/native-core проба-ZfguSe`. The owned
miniature source/build used the rebuilt native launcher and official executable:
PATH discovery skipped its harness build, preview created no homes, install and
update passed runtime acceptance, a copied harness executable was rejected in
both preview and actual update without changing metadata/launch bytes, and
update retained the selected upstream with an empty PATH. Check rejected a
missing Process PATH entry without repair, then passed with the installed bin
directory. Disconnect preserved the original package/source and left the parent
process/User PATH unchanged. This is an outside-checkout core fixture, not
global activation or complete component migration.

The earlier `upstream-real-cli.stdout` failure was the test expecting Check to
succeed with an empty Process PATH. The refusal was correct; the final case
retains it as a negative check and supplies the installed bin entry for success.

`upstream-core-suite.stdout` records 185 passing library tests, with 26 explicit
cases ignored. After additional middle-authored tests and correction of their
three setup/expectation errors, `upstream-middle-cases-final.stdout` records
eight passing resolver tests and one explicit real-package test ignored. They
exercise nested/hoisted/vendor resolution, broken platform refusal, PATH order,
copied/dangling harness exclusions, unknown shims, input bounds, both forms of
ambiguity and package-manager ownership/hint precedence. The first failed test
output is retained separately. Workspace formatting and Clippy for both native
packages/all targets passed after a test-only redundant borrow was removed.
`upstream-acceptance.json` records source/binary identities and command evidence.

## Remaining work

Complete component orchestration and its combined recovery remain open. Native
journal recovery, standalone old core-journal rollback and disconnection of
stable legacy metadata are implemented. The complete legacy compatibility
matrix still requires acceptance; the recovery-preview increment is described above.
Native core PATH now supports User by default and optional Process for the
current manager command. Explicit retirement of absent obsolete entries is
described above. Final private-state cleanup remains unfinished. Management-time
package discovery is described above. Generated launch files and historical
orphaned receipts are retained in private registration state; current operation
receipts have the atomic retirement described above.
The ordinary global installation and its running MCP/subscription services have
not been switched by this increment.
