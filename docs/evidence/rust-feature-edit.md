# Native feature preparation and observation

2026-09-09. [`feature_edit`](../../crates/harness-core/src/feature_edit.rs) ports
the preparation part of `Set-HarnessNativeFeature` for migration task 4.1.
It accepts only the current Hooks/CodeMode operations and an explicitly resolved
original native `.exe`. The installed CLI 0.153.4 `features --help` confirms
native `enable`, `disable` and `list`; no owned script or TOML reimplementation
is needed for those operations.

The caller's bytes are staged in a new owned `CODEX_HOME`. Native editing and
effective-state observation reuse the build manager's existing bounded runner:
512 MiB job, 50% CPU, shared command deadline, bounded stdout/stderr and complete
owned-child cleanup. A result requires natural exit zero, exactly one matching
feature row, bounded output/configuration and the same upstream executable hash.
Duplicate, missing, truncated or wrong feature observations cannot pass.

The result contains candidate bytes and before/after hashes. Debug formatting
and ordinary failures do not emit the TOML or native stderr; private intermediate
files and original process receipts remain under the returned evidence path.
Preparation itself never publishes. The explicit `publish_to` operation now uses
an [existing configuration snapshot](rust-config-file.md) to verify original
bytes and object identity and commit one file atomically. `plan_for` can instead
bind the accepted candidate to the native registration journal before mutation.
Full installer orchestration remains open in tasks 4.1/4.2.

[`feature_edit` acceptance](../../crates/harness-core/tests/feature_edit.rs)
exercises the actual installed Codex executable in separate owned homes:
hooks false, unchanged repeated application, Code Mode true, empty configuration
and malformed TOML. The first three preserve exact comments, Astra model/effort,
route, Goals, disabled memories, nested experimental context and project trust.
The malformed input exits 1 and retains private diagnostics; the public error
contains the failure class and evidence location, without its sentinel text.
The test confirms Debug output does not expose configuration fields.

The native executable hash is
`444A3F0008050605CAE73CD9B7A2DCAC61294062DFAAB56DD20430FD6498518B`.
The initial actual acceptance passed in 52.30 s in the debug test build and
left the global base-config hash unchanged. This is not a startup-speed claim:
the test repeats identity hashing of a 295,408,944-byte native executable for
independent cases. Two model-free unit checks cover exact observation and
invalid path/input privacy. Scoped Clippy passed, and the existing adversarial
finalizer test passed after the runner was exposed to this second consumer.
No runner implementation or build handoff protocol changed.

Evidence root:
`%LOCALAPPDATA%/codex-harness-evidence/feature-edit-a60a84b510704ce6a8b7f2a037bc2c2b/`.
After preserving the concrete failure class in the public error, the affected
native acceptance passed again in 51.19 s. Its four accepted homes and the
malformed-input home are linked from `final-native.stdout.txt`; process receipts
confirm natural success or the expected exit 1, with zero remaining job members.

Run from this repository with `HARNESS_NATIVE_CODEX` set to the verified absolute
upstream executable:

```text
cargo test -p harness-core --lib feature_edit --offline --locked --jobs 1
cargo test -p harness-core --test feature_edit --offline --locked --jobs 1 -- --ignored --nocapture
cargo test -p harness-core --lib actual_finalizer_failures_cannot_supply_an_accepted_record --offline --locked --jobs 1
cargo clippy -p harness-core --lib --tests --offline --locked --jobs 1 -- -D warnings
```

The explicit ignored acceptance is model-free, uses no authentication copy and
preserves its preparation homes. The later publication acceptance passed in
55.29 s, including actual candidate publication and rollback in an owned config;
see `publication-native.stdout.txt`. The final native installer must integrate
these operations with its lifecycle before retiring the script path. The later
native test also passed journaled publication/disconnect in 55.36 s; the retained
`journal-native.stdout.txt` records that run.

The subsequent [new-configuration journal acceptance](rust-config-creation.md)
passed in 50.01 s. It uses the real CLI's empty-input candidate to create an
absent owned config through `apply_with_files`, then disconnects and verifies
removal. Existing-file publication, rollback and malformed-input privacy remain
covered by the same run.

`discover_features` supplies the native `Get-HarnessNativeFeatures` counterpart
for the current Hooks/CodeMode selection. It runs `features list` against the
explicit existing home from a separate owned working directory, with the same
bounded process runner. It neither stages an authentication copy nor issues an
edit command. Both selected feature rows must be complete and unique. A changed
upstream executable, changed base-file bytes/identity, or a config appearing
during an initially config-free observation prevents an accepted result.
This checks the base config; it is not an atomic snapshot of every external
resource referenced by the native configuration.

Actual CLI 0.153.4 observation passed in 26.63 s for two owned homes with opposite
feature values and a malformed TOML case. The intended home's values were returned,
config and unrelated bytes survived, and the malformed input's private sentinel
did not reach the public failure. Missing homes were refused without creation.
A separate config-free home passed in 10.08 s and remained without `config.toml`.
The parser and invalid-deadline unit cases passed, as did scoped Clippy and fmt.
These are model-free execution times, not performance comparisons.

```text
cargo test -p harness-core --test feature_discovery --offline --locked --jobs 1 -- --ignored --nocapture
```

Set `HARNESS_NATIVE_CODEX` to the same explicit upstream executable. Evidence is
under `%LOCALAPPDATA%/codex-harness-evidence/feature-discovery-161c96be09dc49cb9c82bb43e87bdcb9/`:
`native.*`, `native-receipt.json`, `empty.*`, `unit.*` and `final-clippy.*`.
The first run's selected source hashes and the global base-config hash stayed
unchanged. Its original source receipt remains distinct from the later added
deadline rejection and config-free test. Installer Check still needs to consume
this native API before the script helper can be retired.
