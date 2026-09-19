# OFAP 5.1-5.3 result: scoped observations, pacing and the benefit gate

Base commit `ff3284f`; worktree `D:\home\sergey-akhalkov\codex-harness-ofap-feedback`
(branch `ofap/feedback-pacing`). Board issue `codex-harness-qr6.1`.

## What changed

- `crates/harness-core/src/scoped_observations.rs` (new). The three allowed
  sources and nothing else: the installed native Codex/GPT limit snapshot (the
  provider-issued `rate_limits` record the CLI writes into its own session
  files, read newest-first, bounded to 8 newest session files and the last
  256 KiB of each), actual provider refusals reported by lead or executor, and
  bounded user-supplied dashboard snapshots (opaque input: only the fields the
  user typed are recorded). `AccountObservation::bounded` rejects a native read
  without a percentage, a refusal that carries one, and any percentage above
  100. `account_view` keeps missing or stale telemetry unknown (`None`, plus a
  visible `stale_ignored` count) instead of zero or unlimited. Records are
  bounded `pacing-observation v1` board comments; `native_limit_read` performs
  local file reads only and issues no provider request.
- `crates/harness-core/src/pacing.rs` (new). Deterministic plan from scoped
  observations: below 70% used keeps the configured limits; 70-89% halves new
  concurrency and the triage batch (at least 1) and leaves reasoning effort
  alone; 90% or more, or an observed refusal, waits for the reset with
  concurrency 1, a `low` effort ceiling and a batch of 1; unknown or stale
  telemetry holds the configured limits and never raises them. Pacing changes
  new work only - `PacingPlan.preserved` lists healthy executors left
  untouched, and a new assignment queues for a slot instead of preempting one.
  Tasks deferred by one account reset are spread by `BURST_STAGGER_SECONDS`
  (120 s) instead of resuming in one burst. Each deviation carries a
  `pacing-decision v1` record with reason, basis and an expiry taken from the
  observation it came from; `pacing-revoke v1` withdraws it, and an expired
  basis returns the scope to the configured limits. There is no background
  scheduler and no model call.
- `crates/harness-core/src/benefit_gate.rs` (new). Matched-comparison gate:
  per-arm delivery time is check + coordination + rework, the tolerance is
  declared in the record, and only unchanged-or-better quality inside that
  tolerance adopts. `default_allowed` treats an item as a proven default only
  when its latest `benefit-gate v1` board record is an adoption, so an
  inconclusive or rejected comparison cannot clear an improvement.
  `parse_comparison_evidence` is the bounded private intake for measured arms.
- `crates/harness-core/src/lib.rs`: the three modules are declared.
- `crates/harness-core/src/board_feedback.rs`: one bd-backed test records
  observations, pacing decisions, a withdrawal and a gate record on a real
  isolated board item and reads them back.
- `.agents/skills/board-workflow/SKILL.md`: the observation, decision, revoke
  and gate record formats, with the source and unknown rules.
- `.agents/skills/team-lead/SKILL.md`: the pacing policy table, the
  no-preemption and burst-spread rules, and the benefit gate before a promoted
  improvement becomes a default.
- `openspec/changes/orchestrate-feedback-and-pacing/design.md`: the open
  dashboard-snapshot question is resolved (user-supplied bounded readings;
  native read only for GPT/Codex).
- `openspec/changes/orchestrate-feedback-and-pacing/tasks.md`: 5.1, 5.2 and
  5.3 checked; stage 4 and 6 tasks remain untouched.

## How verified

- `cargo test --locked -p harness-core --lib --jobs 1 -- --test-threads=1`
  (worktree, revision `ff3284f` plus this change): 695 passed, 1 failed, 41
  ignored. The single failure
  (`orchestration_lifecycle::tests::preview_check_does_not_write_and_disconnect_keeps_stopped_checkpoints`)
  is pre-existing at the base commit: that test writes only `xai.config.toml`
  while `global/orchestration.toml` requires the `zai` and `ds` profiles, and
  it fails identically on a clean `ff3284f` checkout. It belongs to the
  succession slice's files and is recorded on `codex-harness-qr6.1` rather than
  edited here.
- New-module tests: 27 passed
  (`scoped_observations::`, `pacing::`, `benefit_gate::`), covering the native
  read (newest record wins, other limit identities are ignored, malformed
  records and local token counts never become readings), unknown and stale
  telemetry, refusal handling, comment round-trips, pressure bands, effort
  ceilings, slot queuing without preemption, reset burst spreading, expiry and
  withdrawal, and the gate outcomes.
- Live native read against the installed CLI contract
  (`HARNESS_PACING_CODEX_HOME=<Codex home>`, ignored by default): under the
  15-minute default bound the newest provider-issued snapshot was already
  stale, so telemetry stayed unknown; with an explicitly wider bound the same
  read returned
  `pacing-observation v1 scope=gpt source=native-limit used=14 resets_at=1790418203 window_minutes=10080 refusals=0 observed_at=1789852570 max_age=3600`
  (age 2037 s). No probe call, no invented percentage.
- Live gate intake from a private evidence file
  (`HARNESS_PACING_COMPARISON=<file>`, ignored by default) printed the gate
  record used below.
- bd-backed round trip on a real isolated board item inside the crate test
  suite (observations, decisions, withdrawal, gate record).
- `cargo fmt --all -- --check` and `cargo clippy -p harness-core --all-targets
  --locked --jobs 1 -- -D warnings` (see the closing section for the exact
  scope that was run).

## Benefit-gate comparison record

Promoted improvement `codex-harness-pvr.5` (worktree lane reuse: reset after an
accepted merge instead of a fresh worktree per task). Declared tolerance 10%.
Both arms run the same installed toolchain and the same command on the same
revision `ff3284f`, sequentially on this machine.

Matched task: reach a green `cargo test --locked -p harness-core --lib --jobs 1
-- --test-threads=1` in the lane that will take the next task. Baseline arm
`fresh-worktree`, candidate arm `lane-reuse`.

| Component | fresh-worktree (baseline) | lane-reuse (candidate) |
| --- | --- | --- |
| build (`cargo test --no-run`) | 210.27 s (cold `target/`) | 0.37 s (warm `target/`) |
| check run | 498.92 s | 625.85 s |
| coordination (`git worktree add` + `remove` vs `reset --hard` + `clean -fd`) | 0.94 s | 0.12 s |
| rework | 0 s (none was needed) | 0 s (none was needed) |
| total | 710.13 s | 626.34 s |

Verdict: `adopt` - quality unchanged, delivery time -11.8% against the
baseline, inside the declared tolerance. The board record is
`benefit-gate v1 item=codex-harness-pvr.5 ... outcome=adopt ...`.

Robustness and limits:

- The check-run phase is ambient-noisy on this shared machine: the same lane
  ran the identical command twice (625.85 s and 519.77 s) while a parallel
  executor was active in a sibling worktree. Using the second lane sample the
  total becomes 520.26 s, i.e. -26.7%. The verdict is the same under both
  pairings, and the conservative first-delivery numbers are the ones gated.
- The strategy effect is the build cache (209.9 s measured difference);
  worktree creation and reset cost under a second each, agreeing with the
  earlier lane measurement (~2 s).
- One matched-task run is measured, on this machine, with `--jobs 1` and an
  unchanged check command. Rework was zero in both arms, so the rework
  component of the accounting is present but not exercised; a rework cycle
  after a first delivery compiles only the crate in both strategies.
- The comparison measures delivery time to a green check, not user-visible
  feature value, and it does not measure disk use (a warm lane keeps GBs of
  `target/`), which is a cost of the adopted strategy.
- Quality: both arms produced the same deterministic test outcome (the single
  pre-existing `orchestration_lifecycle` failure). The fresh arm's first run
  additionally failed `native_launcher::tests::run_reaps_detached_session_helper_and_returns_exit_code`
  ("detached session helper survived launcher return"), an environment-
  dependent reaping test that passed in isolation in the same worktree and in
  the lane run; the second fresh run is included in the evidence below.
- Implementation of the lane-reuse policy in `task_worktree.rs` remains
  `codex-harness-pvr.5` (stage 6). This task gates the default; it does not
  implement it and does not close the item.

## Remaining limits

- Pacing is applied by the lead session through the `team-lead` and
  `board-workflow` command tables; the Rust plan is the verified definition of
  those rules. Controller-side dispatch (`task_orchestrate.rs`,
  `executor_cli.rs`) is not changed here - those files belong to the
  succession slice - and the needed change is recorded on
  `codex-harness-qr6.1`.
- Only the Codex/GPT native limit is read from the installed contract; other
  providers pace from refusals and user-supplied snapshots. Local request or
  token counts are never used as a remainder, and no measurement replaces a
  provider statement.
- The freshness bound is a policy input (default 900 s); a lead that accepts an
  older snapshot must state the wider bound, and the record keeps the age
  visible.
- Stage 6 (lifecycle, owning-record updates, end-to-end acceptance) is not
  started.

## Evidence

Private logs and measurements stay outside the pack, in
`%TEMP%\ofap5-*` on this machine: `lane-test.log`, `lane-test-final.log`,
`fresh-norun.log`, `fresh-test.log`, `fresh-test-2.log`, `live-native-read.log`,
`lane-new-modules.log`, `measurements.jsonl` and the comparison evidence file.
The fresh-worktree arm ran in a throwaway checkout
`D:\home\sergey-akhalkov\ofap5-fresh-<stamp>\repo`, removed after the
measurement.
