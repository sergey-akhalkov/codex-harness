# Native user PATH publication

2026-09-09. [environment_path.rs](../../crates/harness-core/src/environment_path.rs)
adds `UserPathSnapshot` and `UserPathChange` for the current user's
`Environment\Path` registry value. Public operations cannot select another key.
The installer must still provide its lock, durable journal and ownership of an
entry before removal. No process environment or application broadcast changes
are made by this primitive.

The snapshot preserves absent versus empty, the REG_SZ/REG_EXPAND_SZ type, exact
UTF-16 bytes and unexpanded variables. Supported strings have one terminating
NUL, valid Unicode and at most 32767 UTF-16 content units. Missing terminators,
embedded NULs, invalid encoding/types and oversized values refuse publication
without normalization. Debug/errors omit contents. The initial malformed fixture
used Win32 string writing outside its documented termination contract and failed;
the corrected Rust fixture stores malformed bytes through the native interface
and independently verifies their preservation.

A single bounded [NtQueryValueKey](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-zwqueryvaluekey)
read obtains raw type/data, avoiding size-query races and string conversion.
The actual opened key name is checked against the current-user root and intended
subkey before value access, refusing registry symbolic-link redirection.

Publication opens/creates the key inside a five-second
[KTM transaction](https://learn.microsoft.com/en-us/windows/win32/api/ktmw32/nf-ktmw32-createtransaction),
stages an uncommitted PATH value, then validates the actual native key name under
write participation. A separate nontransacted handle reads the committed original
type/bytes for comparison, rather than reading our staged candidate. It then
writes/verifies the final desired value, closes the key and commits. An absent
candidate uses a temporary empty value that is always removed or aborted; no
early exit commits it. Rollback applies the reverse exact-value comparison. A
matching desired value is an idempotent no-op; different foreign bytes or type
are preserved. This proves value equality, not the identity/history of its writer.
There is no nontransactional fallback when TxR fails or is unavailable. After a
successful absent-key publication, rollback removes the value and preserves the
key; it never recursively deletes registry state.

The initial twelve owned-key tests passed: absent/empty/types/Unicode round trips, planner
ownership/no-op preservation, foreign byte/type conflicts before apply/rollback,
malformed and oversized data, failure before commit, competing nontransactional
write, registry link redirection, rename/name binding, original-value reads after
staging, same-value write participation, first-stage conflicts and killed Rust
child processes. In the observed
concurrent-write case the foreign writer succeeded, our commit failed and foreign
bytes remained. Existing/absent-key kill cases reached the verified transactional
write boundary; the parent killed/reaped the exact descendant-free child before
its 20-second fallback. Recovery was the kernel's abort, not Rust Drop.

Every mutation used a unique leaf under `HKCU\Software\CodexHarnessAcceptance`.
Cleanup opens the exact leaf with OPEN_LINK and deletes its captured handle;
it never follows a registry link into its target or deletes the shared root.
Two opt-in tests remain ignored by default: the child fixture and actual user
observation. The latter was explicitly run through `UserPathSnapshot::read` and
verified unchanged original bytes; its private representation SHA-256 was
`191d0fb895a8774b9fea7609c7d8b142c479c51bc55eea000b083a20ce3ba6d8`.
Core Clippy, scoped formatting and explicit Serena diagnostics passed.

```text
cargo test -p harness-core --lib environment_path --offline --locked --jobs 1 --target-dir <owned-check-target> -- --test-threads=1 --nocapture
cargo test -p harness-core --lib environment_path::tests::actual_user_path_snapshot_is_readonly --offline --locked --jobs 1 --target-dir <owned-check-target> -- --ignored --exact --test-threads=1 --nocapture
cargo clippy -p harness-core --lib --tests --offline --locked --jobs 1 --target-dir <owned-check-target> -- -D warnings
```

Evidence and exact source hashes:
`%LOCALAPPDATA%/codex-harness-evidence/environment-path-4ed2edb2d8674195a5a6b428bc5cb391/`.
`tests.*` retains the original malformed-fixture failures; `raw-fixed.*` retains
an unrelated incomplete concurrent test-module compile failure. The initial
`coherent.*`, `actual.*` and `clippy.*` results predate the rename correction.
`enlisted.*`, `enlisted-actual.*`, `enlisted-clippy.*` and the current
`acceptance.json` identify acceptance of the rename correction. The subsequent
shared-transaction receipt acceptance is recorded below.
Registration journal integration, the installer lifecycle and global activation
remain unfinished. No real user PATH value was written during this acceptance.

## Rename correction and review

The original implementation checked the key's native name only before its first
value operation. A real owned-key probe found that RegRenameKey succeeded after
opening or reading the transacted key, and a later write/commit landed at the
renamed key. Reading alone did not prevent this. Renaming after a transactional
write also succeeded, but commit then failed with error 6704 and original data
survived. The failing source and `rename-checked.*`, `rename-phases.*` and
`rename-read-enlist.*` preserve that evidence.

The correction relies on tested write participation before the final name check,
including when staged bytes equal the original. Committed original bytes remain
readable separately without aborting the transaction. The actual exchange now
rejects rename at eight checkpoints across publication, rollback and no-op paths,
and preserves foreign writes before/after its first stage. Exact killed-process
and all earlier cases passed again. The public read-only snapshot was rerun and
retained the same representation hash above. This does not prove registry value
writer history; the receipt below proves a committed step, while the installer's
durable intent must still govern ownership/recovery.

A Grok middle review confirmed the original rename defect and found no second
independent production issue in its inspected version. Parent supplied the
runtime counterexample, implemented the correction and verified its actual
exchange entry point. That review preceded the correction; it is not a claim
of a second review of the final source.

## Atomic operation receipts

[environment_path_receipt.rs](../../crates/harness-core/src/environment_path_receipt.rs)
adds a private `PathReceipts` consumer of an immutable `ConfigSnapshot` intent.
It writes `path-applied.json` or `path-undone.json` in that intent's directory.
The receipt's staged NTFS file and registry write share one borrowed KTM
transaction handle, as supported by the [Windows transaction contract](https://learn.microsoft.com/en-us/windows/win32/ktm/programming-model).
The observed commit publishes both; abort or a killed process before commit
preserves both original resources. No fallback writes are used.

Each receipt binds its own file ID/path, the guarded intent's file ID/path/hash,
the exact change hash and current user's native registry location. An undo
receipt additionally binds the applied receipt's ID/path/hash. Both regular
files stay guarded during rollback. Copies, malformed/oversized receipts,
changed intents, other changes and other registry scopes are refused without
changing foreign data. Receipts contain private identifiers and have no Debug
output. Their format is schema 1; registration journal schema 8 is unchanged.

First publication requires the exact recorded baseline. An identical candidate
written by somebody else without our receipt is a conflict. Repetition requires
a valid receipt and the corresponding current value. Undo without an applied
receipt is refused; reapplying an already undone operation is refused. A no-op
with an absent value removes its temporary staged placeholder before committing
the receipt. Equality after our committed step still cannot establish every
later writer's history.

The current suite passed **20 tests**, including the earlier twelve and eight
receipt cases. It exercised eight exact child kills across apply/undo,
before/after commit and absent/present original values, then reloaded the intent
and repeated the operation. Three tests are ignored by default: two child
fixtures invoked explicitly by their parent tests and the actual user read-only
observation. All writes remained under owned acceptance keys and temporary
directories. Core Clippy, scoped Rust formatting and explicit Serena diagnostics
for both PATH modules passed. No global PATH write or power-loss test was run.

An independent Astra senior review inspected the four changed Rust modules,
test bodies and retained results, and found no concrete P0/P1 defect in this
scope. It ran no additional Cargo commands and made no source changes. Its
acceptance retains the caller/lifecycle and writer-history limits below.

Evidence:
`%LOCALAPPDATA%/codex-harness-evidence/path-receipt-9b88ccfe703f4033a5926cbf930ef9fa/`.
`mixed.*` proves shared file/registry commit and abort; `receipts-kill.*` records
the full current suite; `clippy-fixed.*` and `acceptance.json` retain checks and
source hashes. Earlier compile/lint failures remain in the same directory.
Commands used the checkout cwd, `--target-dir` pointing to the existing owned
parent check target, and `-j 1`: `cargo test -p harness-core --lib
environment_path::tests -- --nocapture --test-threads=1` and `cargo clippy -p
harness-core --lib --tests -- -D warnings` (target/jobs flags precede `--`).

The installer must still persist the serialized change in its immutable intent,
hold its installation lock, coordinate this step with registration/metadata and
retire receipts only after its recovery decision. Those lifecycle consumers,
receipt cleanup and global activation remain unfinished.
