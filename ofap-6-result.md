# OFAP 6.1-6.3 + worktree lane reuse (executor `ds`, lane `ofap/stage6`)

Base commit `bd912aa`; the `ofap/stage6` lane worktree. Board issue
`codex-harness-pvr.1`; item `codex-harness-pvr.5`. No board issue was closed.

## What changed

### Worktree lane reuse (`codex-harness-pvr.5`)

- `crates/harness-core/src/task_worktree.rs`: new `LaneDisposition::{Reused, Preserved}`
  and `reset_for_reuse(mapping, current, base)`. After an accepted merge the lane is
  reset with `git reset --hard <base>` plus `git clean -fd`, so ignored build caches
  stay warm, and the result is verified (HEAD at the merged base, clean status) before
  it is reported as `Reused { base }`. A lane that is missing, archived, not a Git
  worktree, without the base commit, or not clean after the reset is reported as
  `Preserved { limitation }` with its reason instead of being deleted or reused
  blindly. `retire()` stays the lane-retirement path; native confirmed deletion remains
  TUI-only, so an unretirable tree is preserved, never silently removed, and merged
  branches are kept.
- `crates/harness-core/src/task_orchestrate.rs`: `merge_accepted` merges the executor
  commit into the lead checkout, resolves the new base and returns the lane disposition
  from `reset_for_reuse` (previously it returned a retirement decision).
- `task_worktree::audit` (the existing lane inventory/limit guard, preserved): the
  guard now normalizes `git worktree list` paths (git prints forward slashes on
  Windows, `Path::canonicalize` returns the verbatim form) and reports an empty
  inventory for a source that is not a Git checkout, so the guard warns about lane
  accumulation instead of failing dispatch with an unrelated `git worktree` error.
  A reused lane adds no worktree, so the configured `worktree_limit` keeps its meaning.

### Lifecycle delivery (6.1)

- `crates/harness-core/src/orchestration_lifecycle.rs` is now wired instead of
  unreferenced:
  - `check(source, codex_home, user_home, preview)` validates the orchestration
    configuration (hard error, unchanged, explicit `profile '...' is not installed`)
    and reports what a fresh session discovers: configured lead/successor/executor
    profiles, vote threshold and worktree limit, the loop guidance skills found in the
    user skill root (`team-lead`, `board-workflow`, with any missing name listed), and
    the board tool version when the board component is connected. All reads; no model
    call and no mutation.
  - `disconnect_preserves_private(codex_home, user_home)` reports which private loop
    evidence a rollback left in place (task store, lane worktrees, delivered skill
    source). A missing optional root is not an error; an unreadable one is.
- `crates/harness-core/src/core_check.rs`: the core check report gains `orchestration`
  (the loop delivery report), so `check --core-only` answers "is the workflow
  delivered?" in one receipt.
- `crates/harness-core/src/core_disconnect.rs`: the disconnect receipt gains
  `orchestration` (what stayed), so rollback states what it preserved.
- `crates/codex-harness/tests/orchestration_isolated_install.rs`: the isolated
  lifecycle acceptance now covers the loop: guidance links exist and their content is
  the current workflow (board markers, vote threshold, board-workflow pointer),
  the orchestration configuration validates against the isolated home's profiles,
  the loop check reports the delivered board version, the installed `bd.exe` runs and
  records real board evidence (epic, task, close with an archive reason) in an owned
  project, and rollback (board Disconnect, core Disconnect) removes the loop links
  while unrelated configuration (base `config.toml` model line, credentials, foreign
  skill) survives and the archived board evidence still reads back. The test also
  plants foreign configuration before install and asserts it is preserved there.
- `crates/codex-harness/tests/executor_spawn.rs`: the fixture now installs the
  configured `ds` executor profile, so `shared_checkout_without_worktrees_is_refused`
  again exercises the refusal it is named for (it failed at the base commit; see
  Gaps).

### Owning records (6.2)

- `docs/agent-delegation.md`: exec-mode tab lifecycle and watcher waiting (one watcher
  event instead of model-side polling or PID polling), a new **Improvement loop**
  section (feedback as board tasks, vote provenance, promotion routing, consequence
  override, hygiene triggers, pacing bands with the 70%/90% thresholds and the
  120-second reset stagger, and the benefit gate), and **Executor worktrees** now
  states lane reuse, the code owner, resettable-versus-preserved outcomes and the
  inventory guard.
- `docs/subscription-models.md`: new **Orchestration spend** section (one process per
  assignment with self-closing exec tabs, watcher waiting, warm lanes, no-model-call
  triage bounded by `feedback_batch_limit`, pacing sources and unknown-is-unknown,
  benefit gate before defaults) with links to the operating owners.
- `docs/token-workflow.md`: orchestration spend accounting (bounded triage without
  model calls, closed exec tabs and bounded successor turns, watcher waiting and lane
  reuse, pacing limits, benefit-gate accounting of check + coordination + rework; byte
  counts still are not weekly-quota claims).
- `docs/project-decisions.md`: the lane-reuse decision now records the implemented
  native path and the adopted benefit-gate outcome, and two confirmed decisions add
  the improvement loop and the pacing/gate/succession policy with links to their
  authoritative homes.
- `openspec/changes/orchestrate-feedback-and-pacing/tasks.md`: 6.1 and 6.2 checked,
  and the accepted 4.1/4.2/5.1-5.3 boxes restored (see Gaps 3).

### End-to-end trail (6.3)

Assembled below; it records only artifacts that exist and are inspectable in this
checkout. The benefit-gate record was transcribed from the stage-5 measured evidence
onto `codex-harness-pvr.5` so the gate is visible on the board.

## How verified

All commands ran in this worktree with PowerShell 7 and the pinned toolchain
(`rustc 1.97.1`). Serial flags match the lanes' earlier runs because the machine
carries parallel load.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo test --locked -p harness-core --lib --jobs 1 -- --test-threads=1` | 711 passed, 0 failed, 41 ignored (336 s) |
| `cargo test --locked -p harness-core --lib board_feedback --jobs 1` | 22 passed, including the `bd`-backed vote, merge, promotion, override, hygiene and gate round trips on real isolated boards |
| `cargo test --locked -p harness-core --lib task_worktree --jobs 1` | 9 passed (lane reset/reuse, preservation, inventory guard) |
| `cargo test --locked -p harness-core --lib task_orchestrate --jobs 1` | 7 passed (merge acceptance resets and reuses the lane) |
| `cargo test --locked -p harness-core --lib orchestration_lifecycle --jobs 1` | 4 passed |
| `cargo test --locked -p codex-harness --bin codex-harness --jobs 1` | 64 passed |
| `cargo test --locked -p codex-harness --test executor_spawn --jobs 1` | 2 passed, 1 ignored (live profile check) |
| `cargo test --locked -p codex-harness --test installation_state --test manager_delivery --jobs 1` | 5 passed |
| `cargo test --locked -p codex-harness --test task_control_contract --jobs 1` | 7 ignored (environment-gated) |
| `cargo test --locked -p codex-harness --test orchestration_isolated_install --jobs 1 -- --ignored` | 1 passed (74 s) with `HARNESS_CONTROL_CODEX_EXE` pointing at the installed native CLI 0.155.1; isolated homes only, model-free |
| `cargo test --release --locked -p codex-harness --test executor_succession --jobs 1 -- --ignored` | 2 passed (succession re-verified at this revision: established with preserved work, deferral while a turn is in flight, `succession not established` on failed reload) |
| `codex-harness ownership-check --source .` | 2 executable files (inert data), 307 scanned Rust files, 0 findings |
| `openspec validate --all --strict` | 31 passed, 0 failed |
| `cargo clippy --locked --workspace --all-targets --jobs 1 -- -D warnings` | fails on 10 pre-existing lints in files this change does not touch (see Gaps 5); no lint in a changed file |

The lane-reuse behavior is covered by native tests that do not need accounts or
network: `lane_reset_reuses_the_checkout_and_keeps_ignored_build_caches` (reset to the
merged base, ignored cache kept, untracked removed, tracked restored, lane clean,
inventory unchanged), `lane_reset_preserves_unresettable_state_for_retirement`
(missing base, current checkout, archived mapping, missing path),
`audit_of_a_non_git_source_has_no_lanes`, and
`lead_merges_executor_commit_into_the_shared_checkout` (accepted merge, then
`Reused { base }` with the lane clean at the merged base).

## End-to-end evidence trail (OFAP stages 1-6, board `codex-harness`)

| Stage | Board record | Commits / artifacts | Status |
| --- | --- | --- | --- |
| 1: verify supporting contracts | `codex-harness-rnm` (closed 2026-09-19T18:14:41Z), `codex-harness-rnm.1` ("OFAP 1.2 resume contract established without picker", closed 18:14:34Z) | No lane commit or result file; evidence in the close reasons, `docs/agent-delegation.md`, `docs/rust-native.md` and private probes under the host `%TEMP%` | accepted |
| 2: feedback intake and triage | `codex-harness-dl6` (closed), `codex-harness-dl6.1` (closed 19:41:31Z), `codex-harness-dl6.2` (closed 21:09:20Z), `dl6.3`/`dl6.4` (acceptance and dispatch) | `87030be` (snapshot with `ofap-213-result.md`), `f1d590c` (integration 2.1-2.3), `0451514` (snapshot with `ofap-24-3-result.md`), `ff3284f` (integration 2.4) | accepted |
| 3: promotion and hygiene | `codex-harness-51w` (closed), `codex-harness-51w.1` (closed 21:09:21Z) | `0451514`, `ff3284f` | accepted |
| 4: instruction-refresh succession | `codex-harness-kon` (closed), `codex-harness-kon.1` (closed 21:30:41Z), `kon.2` (acceptance/merge) | `21dca18` (executor commit with `ofap-41-42-result.md`, tracked), `50fa2e9` (merge into integrated main); this stage re-ran the acceptance suite (2 passed) | accepted, re-verified |
| 5: pacing and benefit gate | `codex-harness-qr6` (closed), `codex-harness-qr6.1` (closed 22:13:29Z), `qr6.2` (dispatch) | `fa8833b` (snapshot with `ofap-5-result.md`), `bd912aa` (integration); benefit-gate record transcribed onto `codex-harness-pvr.5` from the stage-5 private measured evidence (`benefit-gate v1 item=codex-harness-pvr.5 improvement=lane-reuse-worktree outcome=adopt quality=unchanged matched=1 tolerance_percent=10.0 baseline_seconds=710.1 candidate_seconds=626.3 regression_percent=-11.8 accounting=check+coordination+rework`) | accepted |
| 6: lifecycle, records, acceptance | `codex-harness-pvr` (epic, open), `codex-harness-pvr.1` (this assignment, `lead_review`), `codex-harness-pvr.5` (implemented here, `lead_review`), `pvr.2`-`pvr.4`, `pvr.6` (closed 22:13:32Z; commits `640bcc8`, `6200b48`, `3b8719d`) | This lane: `task_worktree.rs`, `task_orchestrate.rs`, `orchestration_lifecycle.rs`, `core_check.rs`, `core_disconnect.rs`, the extended isolated lifecycle test, the owning records and this file | delivered for review |
| Loop feedback (real run) | `codex-harness-pvr.7` - `pvr.11`: five `feedback` tasks recorded 2026-09-19T22:35Z by the lead, episode `ofap-2026-09-19` (`kind: lead`), open and not yet triaged | Board records only | intake exercised |

The loop's mechanics behind that trail are exercised against the real `bd` binary in
the crate suite on isolated project boards: bounded feedback intake and listing,
batch triage with one vote per distinct episode/reporter, repeat and diagnostic vote
rejection with visible provenance, threshold promotion to the backlog, consequence
routing to OpenSpec, kit-concern promotion without private data, the lead consequence
override, the deterministic hygiene sweep with restore, and the pacing/decsision/gate
record round trips.

## Gaps (named honestly)

1. **No vote-based promotion in the real run.** `codex-harness-pvr.5` was created
   directly from an explicit user decision (recorded verbatim in its description,
   `created_by` the user) and its adoption was gated by the measured benefit-gate
   record; the board carries no `feedback-vote` or `feedback-promote` comments. The
   deduplication, vote, promotion, override and hygiene mechanics are verified against
   real `bd` in isolated project boards (22 tests) and documented in the skills, but
   the run did not promote an item through votes. The five live `feedback` tasks
   (`pvr.7`-`pvr.11`) are intake only; their triage had not happened when this file was
   written.
2. **Result-file coverage is uneven.** Stages 2 and 3 have result files only inside
   their lane snapshot commits (`87030be`, `0451514`), not in the current tree, because
   the lane worktree was reset for its next task - the reuse behavior this change
   implements. Stage 1 has no result file at all: its evidence is the board acceptance
   reasons, the owning documents and private host `%TEMP%` probes.
3. **`tasks.md` checkboxes were reverted by the integration merge.** The stage-4 lane
   checked 4.1/4.2 (`21dca18`), and the stage-5 lane checked 5.1-5.3 in its working
   tree, but the lead's integration commit `bd912aa` rewrote that section unchecked.
   This is the regression the lead recorded as feedback `codex-harness-pvr.9`. This
   lane restored the accepted boxes; the underlying result files and close reasons are
   the evidence, and the boxes are not a re-verification of that work.
4. **The stage-5 gate record was not on the board.** The measured adoption existed only
   in the stage-5 result file and private evidence; this lane transcribed the verbatim
   record onto `codex-harness-pvr.5`, marked as transcribed, so the gate is inspectable
   where the skill's format expects it.
5. **Pre-existing clippy failures.** `cargo clippy -D warnings` fails on 10 lints in
   unmodified files (`task_observer.rs`, `benefit_gate.rs`, `native_launcher.rs`,
   `orchestration_config.rs`, `pacing.rs`, `scoped_observations.rs`, `task_runtime.rs`).
   They are outside this assignment's ownership and are recorded as a diagnostic
   feedback item on the board rather than fixed here. The `executor_spawn` fixture bug
   that made `shared_checkout_without_worktrees_is_refused` fail at the base commit was
   in a file this change owns and is fixed.

## Remaining limits

- Lane reuse discards uncommitted lane state by design (`git reset --hard` plus
  `git clean -fd`); only ignored build caches survive. A lane that cannot be reset is
  preserved with its reason, and native deletion remains TUI-only, so nothing is
  deleted automatically and the lead keeps the retirement decision.
- `merge_accepted`/`reset_for_reuse` are library paths with native coverage; the lead
  still performs the merge itself and no CLI verb exposes them yet (`executor_cli.rs`
  is a stable, read-only reference in this assignment).
- The loop check reports the board tool but deliberately does not fail `check
  --core-only` when the board component is absent: the board owns its own check, and a
  missing board stays an explicit limitation instead of a substitute.
- The isolated lifecycle check is environment-gated (`HARNESS_CONTROL_CODEX_EXE`, a
  local native Codex executable) and model-free: it verifies delivery, discovery and
  rollback, not a model-backed conversation.
- Private evidence stays outside Git in host-local `%TEMP%` directories: the stage-5
  measurements with the comparison evidence, the succession acceptance run and this
  stage's command output.
