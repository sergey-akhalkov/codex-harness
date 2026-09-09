# Native registration commitment and cleanup

2026-09-09. [Registration::finish](../../crates/harness-core/src/registration_finish.rs)
adds an irreversible decision followed by recoverable cleanup. The caller must
nominate a metadata configuration change or creation already in the journal.
Arbitrary ordinary files do not authorize commitment. Installation locking and
the meaning of that metadata remain the installer's responsibility.

Before commitment, finish retains guards for every published candidate and old
backup, verifies exact object identities/bytes/targets, and rejects namespace
overlap. It creates `commit.json` inside a transaction, embeds its own identity,
the exact bounded journal/completion and metadata witness, and makes the complete
decision visible in one transaction commit. Any error after that boundary must
be interpreted by inspecting state, never by assuming rollback is still allowed.

Recovery checks the decision before loading ordinary undo state. Cleanup keeps
the published objects guarded, removes only captured backup handles, then the
completion marker, original journal and finally the decision. The embedded intent
allows restart after any of those deletions. Missing published data is a conflict;
only cleanup objects may already be absent. Foreign replacement, changed bytes,
equivalent-alias retargeting or malformed/rebound decisions preserve remaining
objects and evidence. No permanent journal chain or copied source data is created.

| Observed state | Behavior |
| --- | --- |
| Interrupted publication, no decision | `recover` undoes the journal |
| Completed publication, no decision | `recover` refuses; explicit `disconnect` undoes it |
| Valid committed decision | `recover`/`disconnect` only finish cleanup and report `committed=true`; another apply refuses until cleanup |
| Fully retired temporary state | Repeated finish/recover/disconnect are empty no-ops with `committed=false` |

## Version boundary

This finish increment was accepted at **schema 7**. The current
[metadata protocol](rust-registration-metadata.md) uses schema 8 to persist
reused-link identities. Earlier review identified that the previous schema 6
reader ignored the new decision file and could still undo accepted candidates
during cleanup. The intent/completion/decision version was therefore advanced,
and older journals remain unsupported and preserved.

An actual probe compiled the captured pre-finish schema 6 reader/writer with the
current supporting native/configuration modules. Its public disconnect rejected
new schema 7 intent and preserved candidate/journal/completion bytes. Conversely,
current entry points rejected valid intent produced by the old writer and
preserved it. This proves the protocol version boundary; it is not a test of an
archived whole binary. The earlier schema 6 finish evidence remains historical.

## Verification

Sequential Windows checks passed: 40 affected library tests, 38 public integration
tests and the explicit reparse-retargeting regression. The default suites retained
six/eight opt-in ignores; these were not counted as passes. Scoped core Clippy and
formatting passed.

Eight owned Rust process-kill boundaries cover uncommitted staging, decision
publication, guarded commitment, both backup deletions, completion deletion,
journal deletion and final retirement. Before commitment explicit disconnect
restores the originals; afterward recovery preserves accepted candidates and
retires temporary state. These checks kill/reap the exact descendant-free child
and do not rely on its Rust destructors. A separate public test performs four
successive real commits that immediately reuse `commit.json`, covering the prior
NTFS name-tunneling risk rather than only a repeated no-op call.

```text
cargo test -p harness-core --lib registration --offline --locked --jobs 1 -- --test-threads=1 --nocapture
cargo test -p harness-core --test registration_finish --test registration --test config_file --test config_creation --test link_changes --offline --locked --jobs 1 -- --test-threads=1 --nocapture
cargo test -p harness-core --lib registration::finish::tests::retargeted_backup_alias_stops_finish_and_committed_cleanup_before_any_deletion --offline --locked --jobs 1 -- --ignored --exact --test-threads=1 --nocapture
```

Exact commands, source hashes, original attempts and current results are in
`%LOCALAPPDATA%/codex-harness-evidence/registration-finish-38337dcb7a164ca1b95049dbdecdd766/`.
`schema7-handoff.txt`, `schema7-sources.json` and `schema7-*` identify current
acceptance; `schema7-compatibility/` retains the captured-reader probe. Parent
review inspected guard retention, decision/cleanup/dispatch and added the repeated
commit test; the schema correction resolved its compatibility finding.

The later [metadata builder](rust-registration-metadata.md) and
[stable ownership consumer](rust-installation-metadata.md) retain actual staged
and reused identities. PATH/lifecycle integration and global activation remain
unfinished. No power-loss or cross-volume atomic-cleanup claim is made.
