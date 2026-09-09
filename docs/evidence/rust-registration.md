# Native registration foundation

2026-09-09. [registration.rs](../../crates/harness-core/src/registration.rs) is a
bounded primitive for OpenSpec tasks 4.1/4.3: journaled direct file and directory
symlinks and existing configuration changes with owned undo/recovery. It does not install, change PATH, mutate
[global/kit.json](../../global/kit.json), replace the PowerShell lifecycle, or
activate a global layout. Package runtime state stays outside the checkout.

The public API is `Registration::open(state)` plus `apply`, `recover` and
`disconnect` over current [`inventory::Link`](../../crates/harness-core/src/inventory.rs)
values. Apply serializes through `ExclusiveFileLock` on an explicit owner-marked
state directory. Transactional staging creates a new object without exposing it;
the immutable `journal.json` records its staging path, volume/file ID,
creation time, target, type and checksums before creation commits. Publication
renames that same object without replacing any destination. A separate
`complete.json` binds the completed operation to the exact intent.
A matching preexisting direct source symlink is reused without claiming creation and
is left in place on disconnect. Foreign regular files, dangling links and
wrong-target links are rejected before mutation. Interrupted own creation remains
recoverable; foreign destination changes, including an identical target on a
different object, preserve both the live path and the journal. Relative paths,
`..`, and reparse ancestors are refused
through the existing inventory/build-identity helpers. A populated unowned state
root is never adopted.

```text
rustfmt --edition 2024 --check crates/harness-core/src/registration.rs crates/harness-core/src/registration_native.rs crates/harness-core/tests/registration.rs
cargo test -p harness-core --lib --test registration --offline --locked --jobs 1 -- --test-threads=1 --nocapture
cargo clippy -p harness-core --lib --tests --offline --locked --jobs 1 -- -D warnings
```

The initial eight Windows integration tests passed, as did Clippy and the scoped
format check. Parent acceptance subsequently reproduced two ownership defects:
an altered `created` flag claimed a preexisting link
(`%TEMP%/harness-rust-registration-ownership-flag-Kb67gM`), and an actual competing
actor's same-target link was deleted after it appeared between intent publication
and creation (`%TEMP%/harness-rust-registration-foreign-create-race-veok5R`).

Both defects are corrected and their counterexamples now pass. The earlier
schema-2 checkpoint protected the creation flag but still lacked object-origin
proof; it is superseded by transactional staging and schema 3.

[registration_native.rs](../../crates/harness-core/src/registration_native.rs)
holds verified Windows objects and ordinary ancestors through creation,
publication and removal. Owner, journal and completion guards prevent concurrent
record replacement or writes. Reads are bounded; unknown metadata, hardlinked
records, changed ownership and unsupported protection refuse mutation. Cleanup
preflights recorded objects, removes only their verified handles, then removes
completion before intent so an interruption can be recovered again.

The native module passed 23 Windows tests. Evidence and source/build hashes:
`%LOCALAPPDATA%/codex-harness-evidence/unlink-native-b7cea6e146884f08b37b756f1643ab4a/`.
They exercise attribute-only reparse writers (sharing alone is insufficient),
mapped writers, POSIX replacement, ancestor redirects, same-target replacement,
uncommitted staging rollback, sibling transactions, file/directory/dangling
links, and cross-volume refusal. Native publication never copies across volumes.

This implementation requires **local NTFS with TxF**. Microsoft documents
[transactional symbolic-link creation](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createsymboliclinktransactedw)
and warns that TxF may be unavailable in future Windows versions. Failure to
obtain this protection is an error; there is no path-deletion fallback. This is
an explicit platform limitation of the current primitive, not a claim of support
for SMB, ReFS or arbitrary Windows filesystems.

Integration coverage: new/repeat file and directory links, matching preexisting reuse,
foreign regular/dangling/wrong-target rejection, interrupted partial create and
recover, external alteration preserving both versions, lock contention, refusal
to adopt a nonempty unowned root, source survival after disconnect, unrelated
files surviving, and relative/escape/reparse ancestor refusal.

Actual subprocess interruption also passed in two owned 512-link installations:
immediately after intent publication and after the first destination became
visible. The test kills and reaps only its Rust fixture process, then calls the
real recovery API. Both leave the source and unrelated file intact, with no
staging or destination links. Initial receipts:
`%TEMP%/harness-rust-registration-process-interrupt-journal-eKqIvJ/` and
`%TEMP%/harness-rust-registration-process-interrupt-first-published-RuDltZ/`.
The second records one published link at interruption. The race test also checks
that removal of the test actor's conflicting link permits a successful recovery
retry without claiming that actor's object.

Combined current-source acceptance is retained under
`%LOCALAPPDATA%/codex-harness-evidence/registration-11d9392bf27e4569994ae9bbaa7e4194/`:
35 core tests and 16 registration tests passed (13.29 s and 9.92 s), with source
hashes unchanged through execution. The two default ignores are the separately
passed second-volume case and the fixture entry invoked by the subprocess test.
Scoped rustfmt and core library/tests Clippy passed. Native primitive acceptance
and registration integration acceptance remain distinct from a full installer.

The subsequent independent integration review identified a P1 in state-path
canonicalization: a junction replacement between checking and resolving an
ancestor could bind the request to another valid installation. State and source
paths now retain their requested absolute namespace for native guarded access.
The acquired state lock prevents a later directory move; a redirected requested
state is refused without touching either installation. This finding was derived
from source review; its narrow pre-resolution race was not newly reproduced.

The same review found overlapping destinations could create an ordinary parent
that prevented recovery. Planning now refuses ancestor/descendant overlaps before
staging, including Windows ordinal case aliases, interior `.` and repeated
separators. The updated integration suite passed **18 tests** (one explicitly
invoked fixture entry ignored), 14.14 s; logs and hashes are under
`review-fixes-*` in the same evidence root. The added dot/separator variants
subsequently passed their focused test, and core library/tests Clippy passed.
The native primitive source and its 23-test evidence remained unchanged.

This does not close tasks 4.1 or 4.3. Installer/update/preview/Check, PATH,
component selectors, relocation, old-layout rollback, launcher registration and
global activation remain open.

The [configuration publication increment](rust-config-file.md) extends the
journal to schema 4 and adds `apply_with_configs`. Original/candidate bytes and
file identities share the immutable intent and its bound completion marker.
The updated 22-test integration suite and actual interrupted configuration
process pass. Previous schema-3 acceptance above remains historical; older
native journal formats fail closed and are preserved. The primitive still does
not provide the complete installation lifecycle.

A later schema-4 public-API review then reproduced destination alias overlap.
Sagan used an existing NTFS 8.3 alias of an owned ordinary parent and applied
the same missing leaf, plus ancestor/descendant pairs in both orders. All three
cases reached publication, returned OS error 183, retained durable intent, and
failed automatic recovery with no foreign actor. The retained probe state is
`%TEMP%/harness-link-alias-11600/`: README, baseline.txt, same/, ancestor-first/,
descendant-first/, diffs and candidate-run logs. The original probe executable
and source snapshots remain; the pre-change debug rlib was later rebuilt, so no
exact original library hash is claimed.

The subsequent correction compared conflict-only names from a guarded
longest-existing ordinary parent plus an uncreated lexical suffix, then repeated
that check against retained staging-parent handles before intent. Requested
source, state and destination paths stayed the mutation authority. The retained
`alias-candidate-run.json` records 3 explicit alias tests passed; `native-candidate-run.json`
records 28 library tests passed, 3 ignored. The default integration suite first
had 21 passed and 1 failed after a diagnostic string change in
preparation_failure_rolls_back_unpublished_links; after correcting only that
assertion, that focused case passed and the 22 default cases are verified across
those runs. `clippy-candidate-run.json` and `format-candidate-run.json` passed. These
are observations from those retained logs, not a reconstructed source receipt
for the exact files that produced them.

Parent then reproduced an 8.3 alias of the recovery state into journal.json.
`state-alias-before.*` in the feature evidence directory and
`%TEMP%/harness-rust-registration-state-8dot3-overlap-GCcjWV/` show apply returning
OS error 183 after creating journal.json, with the test panicking that the alias
produced recovery intent. The retained GCcjWV journal records a created file link at
the short-name state path targeting the fixture source. The later correction
resolves the retained owner-parent name for conflict checks, rejects planned
config/link and staged destination overlaps with that state name before intent,
and keeps requested paths as mutation authority. `state-alias-after.stdout.txt`
records 4 explicit alias tests passed; the after fixture
`%TEMP%/harness-rust-registration-state-8dot3-overlap-rQMlhb/` has owner and lock
only, no journal. `state-alias-default.stdout.txt` records 22 passed, 5 ignored in
23.50 s. A focused destination_tests fixture exists at
`%TEMP%/harness-staged-destination-conflict-VK2sID/` with only the preserved source
bytes. No independent review of this final state-alias correction is recorded;
parent source review plus these regressions are the evidence. Earlier native
process and feature CLI logs were reused, not rerun.

This schema-4 alias boundary is historical relative to later schema-5 new-file
work in [native configuration creation](rust-config-creation.md). Tasks 4.1 and
4.3 remain incomplete.

The later [link-change increment](rust-link-changes.md) extends the journal to
schema 6 for explicitly captured existing links. It verifies replacement/removal
and recovery of original objects, including real interruption. The subsequent
[finish protocol](rust-registration-finish.md) introduced schema 7 to commit a nominated
metadata record and retire rollback state without allowing an older reader to
undo committed data. The current [schema 8 builder](rust-registration-metadata.md)
also binds reused identities for the [stable ownership consumer](rust-installation-metadata.md).
Full installer integration remains open.
