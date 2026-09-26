## Context

See [proposal.md](proposal.md). The confirmed everyday outcome is ordinary installed `codex` use across projects, optional visible pooled delegation, short commands for questions/replies, and recoverable operation without longer prompts or manual setup. Multiple sessions and shared services must coexist; closing one conversation must not destroy another's work.

The 2026-09-24 audit inspected Codex CLI 0.156.1 and RTK 0.48.0. Codex help exposes profiles, managed worktrees, queue, agents, doctor and app-server. The [official protocol](https://developers.openai.com/codex/app-server) exposes native thread/turn state, input, cancellation, skills discovery and configuration reads. These establish available interfaces, not equivalence to the kit's rules. Source already uses native operations in executor control, structured runs and source diagnostics; those adapters are not automatically redundant.

The audit found only test consumers for `task_orchestrate::plan_executors`, its old `steer` constructor, `skill_evolution::catalogue::build`, `delivery::compact` and the inspected benefit-gate evaluator. This is a bounded workspace observation, not proof that every function in their modules is removable. In particular, executor recovery calls `task_orchestrate::resume`, and CPU admission consumes facilities in `task_runtime`. New core registration defaults `task_control` to false; that does not authorize deletion of an accepted succession requirement or an installed compatibility route.

## Goals / Non-Goals

**Goals:** Native tools own capabilities they already provide; the kit owns only verified missing rules. Remove replaced paths in the same delivery, reduce both maintained production code and instructional load, and retain an executable ordinary user path throughout the migration.

**Non-Goals:** Change the live-link installation contract, break supported public commands, relax worktree/visibility/CPU constraints, change provider or billing, adopt another orchestrator, enable recursive delegation, minify source, or replace automated checks with agent rituals. No new general abstraction, measurement service, registry or report format is needed.

## Decisions

### 1. One change owns each requirement and runtime decision

| Owner | Scope retained by that owner | Composition rule |
| --- | --- | --- |
| `add-observed-executor-tui` | Native presentation and the full modified `Observable executor lifecycle and bounded result` baseline | Complete the native vertical path first; remove the replaced renderer when equivalent presentation is verified. |
| `add-executor-lead-messaging` | Origin/reply binding and additive waiting/request observation | Extend the base lifecycle through uniquely named requirements; do not replace its full requirement block or create a second event/delivery engine. |
| `close-failed-executor-tab` | Terminal host closure and oversized-final recovery | Preserve completed work and the distinction between host exit and assignment outcome. |
| `stop-executor-cache-loss` | Cache-loss stop and fresh-conversation recovery | Preserve its triggers and partial-work guarantees on every retained execution route. |
| `add-shared-agent-cpu-budget` | Aggregate allowance, exceptions, degraded availability and process coverage | Reuse its CPU owner; thread limits are not CPU limits. Preserve its explicitly accepted warned fail-open bootstrap policy. |
| `fix-xai-executor-shim-startup` | Provider transport readiness independent of another session | Keep the shared shim outside one executor's termination tree; no shim retirement without replacement evidence. |
| This change | Remaining native reuse, removal decisions, compatibility, instruction reduction and integrated acceptance | Consume the above outcomes rather than duplicate their tasks or reopen their completed checkboxes. |

The TUI change can be implemented without messaging: it owns the base observation contract. Messaging then adds a live reply hold. A completed native turn with an unresolved request is not a completed assignment, an empty-output defect, permission to close its TUI or permission to release its slot. The additive waiting requirement owns prompt observation of that state; ordinary watch completion/timeout meanings remain owned by the base lifecycle. Final integration checks their composition, including a lead waiting inside watch.

Different changes may prepare independent work concurrently, but edits to common executor, launcher, process and instruction owners must integrate against the current source. Archive/sync the unique requirement operations without overwriting another change's scenarios. Completed closure/cache work remains completed; newly required cross-feature acceptance stays unchecked here.

### 2. Native sessions and events; bounded residual executor state

Use the verified native thread/turn APIs and native TUI. Keep only state native Codex does not own: slot and source/base binding, assignment authority, exact process/run identity, required visible attachment, reply relationships/holds, bounded review result and integration disposition. Keep native history authoritative for conversation content; do not add transcript mirrors or replay models to infer state.

Compare the existing per-run app-server with the native shared daemon using the existing canned provider and native contract checks: exact profile/provider binding, two-client attachment before assignment, active/idle input, reconnect, CPU admission, and one run's stop/view loss without peer damage. Adopt the shared daemon only if these properties hold with less retained machinery and no extra ordinary setup. Otherwise retain the existing isolated backend with a recorded specific gap. Neither topology is a new user operating restriction; both must satisfy the same requirements.

Reuse the TUI and messaging owners' chosen native operations. `queue` acceptance is not model delivery, `turn/interrupt` is not proof of descendant cleanup, and `--worktree` is not a bounded reusable slot pool. Retain adapters for these gaps. Do not enable the unfinished general task controller just to obtain messaging or visibility. Remove unused general-controller branches only after mapping each accepted succession/recovery rule and supported entrypoint to a retained implementation or its still-open owner. A module shared with CPU admission or live recovery cannot be deleted wholesale.

### 3. Reuse native configuration and discovery without losing metadata

Use native reasoning configuration in examples and assignment interfaces. Retire the recommended `routine/standard/demanding` vocabulary; keep the existing alias as a minimal compatibility translation while that spelling remains supported. Preserve invalid-input and explicit profile/native override behavior. This is not approval to delete a public flag or alter effective effort.

Use native `skills/list` and its refresh/invalidation contract for effective selection; retain only canonical identity/revision, conflict, disablement and incomplete-coverage information missing from the native response. Discovery, body access and model awareness stay separate. Preserve current-turn, compaction, resume and child-awareness acceptance; do not infer it from `skills/changed`. Remove the unconsumed catalogue/delivery helpers when caller and requirement checks establish their replacement. Keep skill publication compare-and-swap, isolation and usage analysis.

Move tool inclusion/exclusion into supported Codex/Serena configuration when the installed versions expose the accepted selection. Verify both initialize and activate-project guidance: filtering `tools/list` alone leaves instructions referring to hidden tools. Reuse upstream contexts/prompts where possible and keep only necessary correction in the existing adapter. This does not replace Serena's project/service broker, Windows process ownership, resource limits or Nuphus browser/desktop guarantees. [Native tool settings](https://learn.chatgpt.com/docs/config-file/config-reference) and [Serena configuration](https://oraios.github.io/serena/02-usage/050_configuration.html) are the contracts to qualify.

### 4. Give the remaining audit candidates explicit dispositions

| Area and source owner | Selected action and bounded decision-changing check |
| --- | --- |
| `source_diagnostics.rs`, `source_diagnostics_view.rs`, `native_read_rpc.rs` | Reuse `codex doctor --json` for overlapping native health facts only if its installed output supplies the needed fact/failure classification. Retain existing native config/skills reads and kit links/build/ownership provenance; do not add a doctor subprocess where the existing native call already suffices. |
| `tools/rtk-adapter/src/main.rs`, `token_workflow_lifecycle.rs` | Qualify the installed RTK Codex hook on the owner's PowerShell 7 path, literal arguments, one execution, stdout/stderr/exit preservation, unsupported formats, raw retention and paged recall. Replace equivalent hook/rewrite mechanics; preserve the smallest adapter for missing guarantees. Native-hook availability alone is not equivalent recall. |
| `dependency_*`, installation/registration/build owners | Continue using native `uv`, package managers and upstream checksums; remove redundant parsing/probing only when existing candidate, trust, transaction and recovery scenarios still pass. Native plugins do not replace live linked sources, immutable active builds or ownership-aware disconnect. Do not introduce another package manager. |
| `xai_responses_shim.rs`, provider login/token/lifecycle owners | Recheck each documented wire adaptation against the explicit current Codex/provider contract with existing synthetic upstream fixtures. Retire only proven obsolete adaptations; direct-route availability requires authorized real-provider evidence as well. Preserve OAuth refresh, cold startup, parallel reuse and recovery; unavailable access means retain, not assumed equivalence. |
| `board_feedback.rs`, `benefit_gate.rs`, `pacing.rs`, `feedback_cli.rs` | Let `bd` own issues, labels, comments and gates. Keep vote identity, deduplication, thresholds, promotion, uncertain retry and budget policy in one consumed path. Trace test-only evaluators to the accepted feedback/skill-evaluation rules; integrate a needed rule through the existing command owner or remove only an unneeded duplicate. No new board mirror and no manual gate-calculation instructions. |
| `structured.rs`, `regression.rs`, `verification_record.rs`, `outcome_*` | Preserve native exec/schema use and independent result/unchanged-input/process evidence. Consolidate repeated process observation only where the same guarantees and a real caller exist; no new generic runner. Keep required outcome/skill comparisons and failure scenarios. |
| `token-audit`, `delegation_usage*`, disk-reclaim and ownership checks | Retain these distinct capabilities. Native usage counters, ordinary filesystem commands or Codex review do not supply their cross-session attribution, bounded safe deletion or repository-specific invariants. No rewrite merely to meet a reduction count. |
| Git memory, OpenSpec, Rust/Cargo and worktree skills | Keep native tools and their existing records. Remove duplicated command mechanics in owned guidance only; protected OpenSpec instructions/schemas and required consumer acceptance remain unchanged. |

Every candidate closes with either actual removal and passing affected checks or a precise retained gap, source/version and exercised evidence in the existing owner. Do not turn this table into a second runtime registry. A retained decision cannot claim the absent benefit, hide an unrun mandatory acceptance check, or close another change's unfinished task.

### 5. Measure less code and less instruction work

Before edits, identify the source revision and relevant dirty inputs. Record counts and chosen equivalent task inputs in the existing verification/decision lifecycle. Count non-test first-party executable source separately from unit/integration tests and fixtures; include newly added maintained helpers. Compare normal formatted source, not minified files, and review dependency/operating burden as part of the decision.

For instructions, include affected owned global/developer text, skill metadata/bodies, required references, generated free-text/structured briefs and mandatory command guidance. Hold user payloads constant. Compare ordinary direct work, delegated dispatch, clarification/reply and interrupted-run recovery. The final combined owned corpus must decrease in UTF-8 bytes and whitespace words, and no scenario's required instruction load may grow. Moving explanations into a required reference, extra command output or a generated brief still counts. Externally maintained content and planning specs are separate; bytes/words do not measure subscription savings.

Reduce `team-lead` and `board-workflow` procedural duplication together with the owning command/help/docs. Retain task selection, authority, acceptance and escalation decisions; keep executable validation in the commands. Do not require a new model-backed benchmark platform: reuse required native acceptance and existing skill-evaluation rules for changed applicability or behavior. Evidence from a synthetic provider qualifies protocol mechanics, not the real consumer's ability to follow shortened instructions.

## Risks / Trade-offs

- Native features change across CLI versions -> qualify the explicit executable and preserve the supported compatibility boundary; reject or retain a concrete unsupported path, without silent fallback to weaker behavior.
- Shared backends change termination and CPU ownership -> compare real concurrent routes before adopting them; never put peers in a single executor's kill tree.
- Unused code represents unfinished accepted work -> retain the rule and its task status, and establish its real consumer before claiming completion.
- Legacy support consumes some code -> preserve required compatibility and recovery; a shorter repository does not authorize breaking installed callers.
- Instructions shrink while mandatory reading grows -> compare the full scenario load, including references and generated output.
- A pure size target encourages lost tests or safeguards -> count production separately, preserve behavior coverage and require an outside-checkout installed path.

## Migration Plan

1. Record the implementation baseline and land safe native-interface/instruction and unused-helper reductions with focused checks; no broad cleanup or specification weakening.
2. Consume the native TUI and lead-message changes, preserving completed terminal/cache fixes and the current CPU/xAI owners. Consolidate only after their actual working paths exist; do not duplicate their acceptance tasks.
3. Resolve every bounded candidate in the table, remove accepted duplicates and update its existing guide/decision. Integrate affected instructions and perform the combined reduction/behavior comparison.
4. Use immutable build/deploy/check/recover/disconnect through the existing lifecycle. Verify installed consumption outside this checkout, concurrent work and relevant failure/recovery cases; old sessions retain their owning build and readable receipts.
5. Reconcile main-spec deltas, active changes, guides and completed tasks only against observed results. Rollback uses the existing previous-build and ownership journals, preserving partial work and later user edits.
