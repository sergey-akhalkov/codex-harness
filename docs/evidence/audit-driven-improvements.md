# Audit-driven harness improvements acceptance

Implementation evidence for the `verify-audit-driven-harness-improvements`
OpenSpec change. This record is unfinished until that change is archived; each
entry names the actual command, revision, scope and limits. Private raw logs,
model-backed inputs and machine-local state stay outside tracked source.

## Implementation base

- Base revision: `4b2f958b88d6667b9eff71c25254cf42834ecdb7` ("Retarget archived
  RTK design link to the harness-rtk crate", 2026-09-29 16:36:52 +0300).
- Dirty inputs at implementation start: untracked planning artifacts in
  `openspec/changes/verify-audit-driven-harness-improvements/`; no tracked-file
  modifications. The planning design rechecked its source observations at this
  same revision, so no audit row was fixed by an intervening change and no item
  needs preservation as "already fixed".
- Toolchain observed 2026-09-29: rustc/cargo 1.98.1, clippy 0.1.98, Windows
  x64. Serena project `codex-harness` activated with `rust` and `python`
  language servers; `get_symbols_overview` on `crates/harness-rtk/src/main.rs`
  succeeded. The planning-time language-server failure did not reproduce in
  this invocation; its unresolved cause remains group 11 work, and this single
  success is not delivery evidence for semantic readiness.
- Small native check: `cargo fmt --all -- --check` passed at the base revision.

## Audit predicate spot-verification

The working tree equals the audited revision, so the design's recheck holds by
identity. Representative predicates were re-read in source on 2026-09-29 at
that revision:

| Row | Current source evidence |
| --- | --- |
| A01 | `pack_store` (`crates/harness-rtk/src/main.rs:610`) writes the pack file before reading the shared index; `retain` (`:586`) deletes `.log` files absent from the in-memory index; `write_pack_index` (`:542`) uses a common `index.json.tmp`. The two-writer loss interleaving is present. |
| N01 | `read_pack_index` (`crates/harness-rtk/src/main.rs:524`) does not validate schema or records and silently drops malformed entries via `filter_map`. |
| A05/A09/N02 | `baseline::save` (`crates/token-audit/src/baseline.rs:120`) derives the snapshot name only from the report timestamp and renames over an existing name; same-second saves collide and overwrite. |
| A19 | `sweep_stale_scratch` (`crates/harness-core/src/native_build.rs:268`) removes prefix-matching directories by age in the process temp root without verified ownership or a live lease. |

## Entry-point and test map

| Change group | Native owners |
| --- | --- |
| 1 RTK | `crates/harness-rtk/src/main.rs`, `crates/codex-harness/tests/rtk_adapter.rs` |
| 2 model/effort | `crates/harness-core/src/portable_config.rs`, `crates/harness-core/src/launcher.rs`, `crates/harness-core/src/native_launcher.rs` and their existing tests |
| 3 baselines | `crates/token-audit/src/baseline.rs`, `crates/token-audit/src/report.rs`, `crates/token-audit/src/main.rs`, `crates/token-audit/tests/` |
| 4 interval/incremental | `crates/harness-core/src/rollout_reader.rs`, `crates/token-audit/src/report.rs`, `crates/codex-harness/tests/delegation_usage.rs` |
| 5 launch integrity | `crates/harness-core/src/build_identity.rs`, `crates/harness-core/src/build_selection.rs`, `crates/harness-core/src/native_build.rs`, `crates/harness-core/src/native_launcher.rs` |
| 6 scratch | `crates/harness-core/src/native_build.rs` and its tests |
| 7 xAI transport | `crates/harness-core/src/native_launcher.rs`, `crates/harness-core/src/xai_responses_shim.rs`, `crates/codex-harness/tests/xai_transport.rs` |
| 8 feedback/publication | existing dev/package/integration routes over `crates/harness-core/src/native_build.rs` and `build_identity.rs` |
| 9 benefit evidence | `crates/harness-core/src/benefit_gate.rs`, `crates/harness-core/src/outcome_report.rs`, `crates/codex-harness/tests/outcome_*.rs` |
| 10 skills/context | `crates/codex-harness/tests/skills_isolate.rs`, `crates/codex-harness/tests/skill_eval_*.rs` and the skill owners they exercise |
| 11 semantic readiness | `crates/harness-core/src/serena.rs`, `crates/harness-core/src/serena_shared.rs`, `crates/codex-harness/tests/serena_stdio.rs` |
| 12 CI | new `.github/workflows` plus the native check commands in `docs/rust-native.md` |
| 13 integration | installation lifecycle, outside-checkout consumers and this record |

## Synthetic input and oracle ownership

- Concurrent storage (RTK packs, baseline snapshots): every new reproduction
  runs under an isolated `tempfile::TempDir` home. RTK tests set `CODEX_HOME`
  to the fixture home through the `rtk_adapter.rs` invoke helpers, and
  token-audit tests pass explicit temporary directories; fixtures never touch
  the real user home. New two-process reproductions must keep this boundary
  and use deterministic synchronization rather than sleeps.
- Transport (xAI/loopback): owned `127.0.0.1` listeners on ephemeral ports
  with synthetic secrets only; no production endpoint or credential is
  contacted by tests.
- Usage (rollout sessions): synthetic session files under temporary roots;
  the real rollout directory is never a test input.
- Oracles: authoritative expected outcomes are compiled assertions in tracked
  test source; candidate binaries cannot rewrite them. Any file a candidate
  writes is compared against the oracle, never trusted as its own expectation.
- Evidence locations: public synthetic commands and outcomes live in this
  tracked record; raw logs, model-backed inputs and machine-local detail stay
  in local temporary or private roots outside the repository.