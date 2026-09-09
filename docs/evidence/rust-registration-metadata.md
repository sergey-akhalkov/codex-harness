# Registration metadata builder and durable reused identities

2026-09-09. The crate-private
[metadata builder](../../crates/harness-core/src/registration_metadata.rs)
reserves an absent file or exact existing snapshot before staging. Its immutable
view contains final paths, stored targets, link types and actual object IDs;
previous reused/replaced/removed IDs; and the metadata file's own reserved ID.
The caller supplies ordinary bytes, which become part of the same journal before
publication. Raw bytes and the aggregate journal are bounded. Errors preserve
uncommitted stages and redact callback details.

The current registration protocol is **schema 8**. Parent review found and the
worker reproduced a stale-ownership defect: after apply, a reused link could be
replaced by another object with the same target, and finish accepted metadata
containing the old ID. The failing source and real fixture are preserved. Reused
IDs are now checksum-bound in the journal and checked by live verification,
finish, committed cleanup and undo. Undo retains the reused guards throughout
its mutations and never deletes those reused objects.

The worker verified 51 affected library tests, 38 public integration tests, both
explicit alias-retargeting cases, Clippy and formatting. Default suites retained
seven/eight opt-in ignores. Ten metadata tests include actual
apply/finish/reload/update, builder refusal and size bounds, and killing the exact
owned Rust child before intent publication. The finish conflict matrix covers
reused and metadata object substitution before and after commitment, preserving
candidates, backups and foreign copies until the exact original is restored.

A separate actual compatibility probe compiled the captured schema 7 reader and
writer with current supporting native/config modules. Old finish/disconnect
reject schema 8 intent, with and without reused IDs. Current entry points reject
valid schema 7 intent and preserve its candidates and records. This is a captured
source compatibility oracle, not an archived whole-binary test.

```text
cargo test -p harness-core --lib registration --offline --locked --jobs 1 --target-dir <owned-target> -- --test-threads=1 --nocapture
cargo test -p harness-core --test registration_finish --test registration --test config_file --test config_creation --test link_changes --offline --locked --jobs 1 --target-dir <owned-target> -- --test-threads=1 --nocapture
```

Exact commands, hashes, original failure and final evidence:
`%LOCALAPPDATA%/codex-harness-evidence/registration-metadata-0d0ef7cc3dbb413190a59b710588a121/`.
`schema8-handoff.txt`, `schema8-sources.json`, `schema8-*` and
`schema8-compatibility/` identify worker acceptance; earlier unprefixed logs
describe the historical schema 7 bridge. The original stale-ID counterexample is
`reused-identity-before.*`, followed by its passing `after` run.

The subsequent [stable ownership consumer](rust-installation-metadata.md) exposed
a sharing violation when it tried to reopen metadata already held by the builder.
The view now also carries the SHA-256 of the exact original metadata bytes
(or absence), so consumers can bind prior semantics to the existing held snapshot
without reopening it. This adds no serialized journal field or schema change.
After that addition, all ten metadata tests were rerun successfully, along with
the six consumer tests and core Clippy. Its separate evidence records that later
source state. CLI orchestration, PATH journal integration and global activation
remain unfinished.
