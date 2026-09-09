# Native manager version handoff

The native build path now transfers finalization to the freshly compiled manager.
This record covers the local build/selection foundation, not global cutover.
The active global script installation and shared subscription proxy were untouched.

## Reproduced failure

`%LOCALAPPDATA%/codex-harness-evidence/update-producer-4c889ee82b54442191819ce9d3eabb3c/`
retains an actual historical release, copied source/build metadata, command output,
and `counterexample.json`. The copied old manager first reported healthy, then
source-stale after current Rust inputs were overlaid. It built and activated a new
candidate with exit 0. Its old Check returned healthy; the actual new executable
returned incompatible and denied both runtime and management. The old producer
copied four binaries and used its old input algorithm; the consumer needed five
and excluded the now-live inspection schema from compiled inputs.

That frozen executable predates the handoff protocol. Source edits cannot change
its running producer algorithm. Existing preprotocol candidates require explicit
current Cargo bootstrap; the new protocol is not a retroactive fix for them.

## Implemented contract

The parent captures native inputs before compilation, retains its installation
lock, and runs one Cargo build in a fresh short target directory. A separate
512 MiB/50% CPU Job executes that target's manager with `finalize-build-v1`.
Finalization has a 60-second deadline and observed output bounds. Its request,
response and logs remain in staging or the published immutable candidate.

The fresh manager verifies its executable/path/hash, disjoint ordinary roots and
owned staging state. Every required current input must match the parent's
pre-compilation map. Removing an obsolete input is permitted; inventing evidence
for a newly required input is refused with an explicit bootstrap action. The
fresh manager checks compiler dep-info for its complete binary set, uses exclusive
file creation, writes its own build record, and runs its strict native Check.
Source changes during compilation or finalization refuse publication.

The response binds the exact request bytes, compiled manager, build receipt,
source identity and healthy verdict. The parent validates stable v1 transport
integrity without substituting its old compiled binary list, rechecks its source
snapshot, and alone publishes staging by rename. Unknown transport versions are
unsupported; this is not a promise to update through arbitrary future schemas.

Explicit reuse invokes the integrity-verified candidate's own model-free Check.
Explicit activation transfers to that manager before the selection lock is
acquired. A known v1 predecessor with a different contract is checked through its
actual verified manager when considering rollback. Unknown predecessor schemas or
fields preserve the pointer for compatible recovery. Ordinary launch and public
Check remain passive: neither path compiles nor starts this handoff.

## Acceptance

The real transition test constructs a protocol-capable four-binary predecessor
with the historical schema-input rule from an owned copy of current Rust sources.
Its initial Cargo bootstrap is separate from the update under test. It then
updates to the five-binary consumer in one Cargo compilation. The fixture uses
unoptimized release code for feedback speed; production-profile build and global
activation are separate migration acceptance.

`%TEMP%/harness-native-transition-wZAYdA/` records the passing transition, actual
new-manager Check, delegated activation, usable predecessor journal and unchanged
reuse. Editing the live schema also reused the new build. The 95.52-second case
also passed interrupted rollback before/after pointer replacement, refusal after
alteration of the previous manager, preservation of a foreign pointer edit, and
successful retry after restoring those owned test inputs. Its
`cross-version-recovery.json` records all five observed outcomes.

All six existing manager/Cargo cases passed with actual finalization/Check
handlers, replacing print-only manager doubles. They cover fresh/reused builds,
source staleness, failed builds preserving selection, repair, same-mtime changed
bytes, compiled resources and omitted dep-info inputs, foreign cache links,
missing prerequisites, foreign state and ambient Cargo configuration.

Thirteen core tests passed, including journal recovery/conflicts and unknown
predecessor metadata. The native adversarial finalizer exercises nine rejection
scenarios: missing/malformed response, exit 7, timeout, output flood, wrong request
binding, changed request, changed receipt and a false healthy verdict. Initial
extended evidence is `%TEMP%/harness-handoff-rejections-LxYMdf/`; subsequent normal
test runs retain their own case roots. An input-map test separately checks removed,
new, changed and invalidly identified input evidence. Scoped Clippy passes.

Independent review found a second P1: an indeterminate predecessor Check was
silently classified as damage, changing rollback into replacement completion.
The actual malformed-response counterexample is retained at
`%TEMP%/.tmpw67vgy/predecessor-result.json`. The corrected classifier permits repair
only for observed missing/changed supported bytes. Unreadable or unsupported
metadata and failed consumer verification abort before journaling. Strict nested
metadata is checked even when the pointer's receipt hash is valid.

Actual malformed-output and timeout refusal cases passed at `%TEMP%/.tmp7qhsBw/`
and `%TEMP%/.tmp2zmjHw/`; both preserve the pointer and binaries without creating a
journal. Confirmed-damage recovery tests still pass. The independent follow-up
confirmed the classifier correction and found no residual P0/P1 in that scope;
the repeated real version transition/recovery passed against the corrected code.
Five native launcher and seven structured consumer cases also passed after the
handoff changes. Task 2.3 is closed using this additional evidence and the earlier
explicit missing/altered-manager bootstrap evidence in [Rust migration](rust-migration.md).
Build preparation is not global installation acceptance.

Commands:

```powershell
cargo test -p harness-core --lib --offline --locked --jobs 1 -- --test-threads=1
cargo test -p codex-harness --test native_build --offline --locked --jobs 1 -- --test-threads=1
cargo clippy -p harness-core --lib -p codex-harness --bin codex-harness --test native_build --offline --locked --jobs 1 -- -D warnings
```
