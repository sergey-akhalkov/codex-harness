---
name: self-improvement-loop
description: Operate a requested continuous harness-improvement loop with proportionate experiment selection, qualified local agent runs, sequential real-task comparisons and evidence-backed adoption. Skip ordinary implementation, a single review, and skill-library usage analysis.
---

# Self-improvement loop

Ordering fixed by the owner: when improving this harness, look first for
complexity to eliminate - redundant stages, entities, copies and machinery -
because removal both shrinks the maintained surface and makes work faster and
cheaper in tokens. That ordering never licenses a quality trade: work
quality, reasoning depth and the ability to close tasks stay maximal in every
arm, and speed or token savings are accepted only on top of unchanged or
better task-closing ability.

The ordering is two-phase. Only after no further removable complexity is
found does the loop propose additions or enhancements, and each such
candidate is judged on all three dimensions together: lower token
consumption, faster work and higher task-closing quality - quality meaning
accepted tasks that do not later demand extensive rework from surfacing
critical P0/P1 defects. An addition that saves tokens or time while leaving
task success or defect behavior unchanged at best is not a benefit claim.

Use the consuming project's Beads board and the native `codex-harness improve`
controller. Read `codex-harness improve --help` for the installed request and
command contract:

```text
codex-harness improve start --run DIRECTORY --spec FILE [--supervision once|continuous]
    [--integration-check FILE] [--successor-spec FILE --successor-run DIRECTORY]
codex-harness improve status --run DIRECTORY [--json]
codex-harness improve select --run DIRECTORY --variant baseline|candidate
codex-harness improve stop   --run DIRECTORY [--reason TEXT]
codex-harness improve resume --run DIRECTORY
codex-harness improve prepare-runtime --source DIRECTORY --state DIRECTORY --output FILE
```

If unavailable, report the missing installed capability and use the kit
installation/update owner; do not replace it with an ad hoc model driver. Keep
endpoint details, raw traces and acceptance inputs in local run storage. The
[native guide](../../../docs/rust-native.md) owns command details. Comparison
execution and frozen-arm preparation stay separate owners: phases that need
them remain pending in `status` until an actual effect records them, and that
pending state is not an operative loop. `start` validates the strict schema-1
run inputs (project, board, OpenSpec change, base revision, writable scope,
runner profile, evidence root, oracle, publication scope and any removal
scope), refuses duplicate run ownership, qualifies the linked OpenSpec change
through the installed CLI and performs model-free preparation; a missing
evidence base records `idle`, a missing dispatch, qualification or removal
gate records `blocked`, and neither starts hidden model work. Changed run
inputs require a new run decision before dependent effects.

New model work is dispatched only from a ready gate: an active attempt or an
attempt with an unknown outcome, a lost conversation surface, a missing or
unusable launcher/profile binding, an incomplete measured-arm qualification,
or a pending, refused or withdrawn removal on a treatment blocks dispatch,
and no hidden fallback is used while the reason is reported.

## Select the smallest sufficient experiment

Choose the experimental unit and method after admitting the hypothesis and
before implementing the candidate. The owners already exist: the run spec's
`experiment` contract, the operator's `measurement-scope.json`, the
hypothesis's own OpenSpec change and workload contracts, and the predeclared
comparison policy file. Do not add a selection framework or a second journal.

Declare, before dependent work:

- the mechanism and falsifiable claim, and the required outcome that would
  support it;
- the experimental unit and method: a real operation or bounded input replay,
  a short agent task, complete paired task implementations, or a sequence
  preserving repeated-use, recovery and state;
- the applicability rationale: why the mechanism applies to this workload
  and operating conditions, and why the chosen unit exercises it and covers
  the decision-relevant failure paths — effect propagation through agent
  choices, relevant variability and confounders, error consequences,
  reversibility, expected frequency and use horizon, and total
  investigation, implementation and experiment cost;
- the controls and operating conditions the comparison needs, including the
  repeatability/identity policy;
- projected use and cost of the experiment and of retaining its work;
- the baseline basis: the accepted revision, what it deliberately excludes,
  and what would make it stale for reuse;
- stopping, escalation and deferral rules, declared before results.

Selection follows the evidence. A short operation or bounded replay is
eligible when it preserves the claimed mechanism and required acceptance —
for example, a local build or output treatment measured through the real
build/check cycle. Whole-task paired implementations are selected when
shorter work would omit decision-relevant strategy, interactions, corrections
or outcomes; a repeated-use or recovery claim preserves the necessary
sequence and state.
There is no mandatory escalation ladder, invented precision score or universal
budget: record why an omitted unit cannot change the declared decision. An
agent-choice claim needs a real agent task — a fixed-command probe or retained
replay cannot stand in for an unexercised agent or end-to-end effect. Fewer
lines, files, skills or exposed names never establish benefit; a subtraction
needs coverage, lost-use, consumption and restoration evidence. Predictions
stay estimates, and the success threshold is not moved after results;
evidence that changes the hypothesis or required scope revises the plan
explicitly before dependent work. When a sufficient experiment is not worth
its cost, defer it with the missing fact and the reconsideration condition
instead of adopting without support, weakening correctness, or repeating an
identically inconclusive run.

## Run the measurement-to-decision lifecycle

The recoverable phase cursor is `planning` → `candidate-ready` →
`baseline-attempt` → `candidate-attempt` → `acceptance` → `decision-recorded`
→ `activation-confirmed`, with `idle`, `blocked` and `stopped` as conditions.
Use `board-workflow` for hypotheses, prior-result search and decisions; its
[comparison record guide](../board-workflow/references/self-improvement.md)
describes ownership.

1. **Investigation.** `improve start` advances a bounded investigation: a
   retained investigator result is consumed through grounded intake against
   retained native evidence — never investigator-supplied labels alone.
   Admit attributable evidence with a mechanism, predicted effect,
   applicability, counterexample and independent acceptance. Consider no
   change, reuse, simplification and subtraction first - and among these,
   look first for removable complexity: a candidate that eliminates
   unnecessary machinery should both reduce the entities the kit maintains
   and make everyday work faster and cheaper in tokens. Only when no further
   removable complexity is found does the loop propose additions, and an
   addition must improve all three together - tokens, speed and task-closing
   quality (fewer later P0/P1 rework demands), not just cost. Quality is
   never the payment for that speed or economy: work quality, reasoning depth
   and the ability to close tasks stay maximal in every arm, and a hypothesis
   that would trade them away is refused at intake no matter how much it
   saves.
   No new basis means reuse the earlier conclusion or remain idle, without
   manufacturing work or votes.
2. **One planning contract per hypothesis.** Each hypothesis — including a
   one-line instruction change — needs its own complete, validated OpenSpec
   change before implementation: proposal, specs, design, tasks, and an
   acceptance section in the artifact the experiment contract names. A
   missing change is scaffolded through the installed CLI inside the
   hypothesis's owned worktree and authored by a bounded planning
   conversation; implementation is dispatched only after qualification
   succeeds (`openspec validate --strict` and the implementation prerequisite
   must pass, and the planning artifacts must not change during
   qualification). A workload that implements B keeps B's own card and change
   separate from the candidate's. Use the installed OpenSpec skills without
   editing their workflow. A reusable procedure in owned skill scope follows
   `skill-evolution`. An ordinary evaluation workload that is not itself a
   hypothesis reuses its existing contract instead of a fabricated change.
3. **Directed initial measurement.** State the `MeasurementScope` JSON
   (observed problem, investigation scope, measurement question, selected
   existing operation and its contract link, evidence references, limits,
   declaration artifact and heading) in the run directory before the first
   targeted baseline measurement. The hypothesis's own change must state the
   same scope section; the controller resolves it through the installed CLI
   and retains the receipt before any baseline direction, and revalidates it
   on later passes. A missing, unstated, incomplete or changed scope blocks
   directed measurement; a changed declaration invalidates a retained
   baseline rather than reusing it under the new one.
4. **Implementation and candidate readiness.** The implementer works from the
   qualified change; the controller validates the returned checkout against
   the exact committed base, the declared writable scope and the frozen
   planning artifacts, and retains `candidate-ready`. Nothing here merges to
   the mainline, records a decision or applies a removal.
5. **Measured pair and acceptance.** With explicit comparison inputs, the
   controller prepares and drives the sequential pair: baseline attempt,
   then candidate attempt, exactly one measured conversation at a time.
   `select` activates an already prepared variant through the shared
   runtime-selection owner, records the identity it actually consumed and
   performs no model call, build or source edit; it refuses while a measured
   attempt is active or unreconciled, and a candidate removal treatment
   additionally needs the current experimental removal authority. Acceptance
   comes from the unchanged oracle referenced by the run, never from
   candidate claims; a recorded decision is not proof that acceptance ran.
   An unknown attempt outcome is never resubmitted.
6. **Decision.** The comparison owner applies the predeclared policy from the
   actual outcome accounting — correctness first, then the declared
   meaningful effect, tolerances, matched units, stopping and
   repeated-selection treatment — and publishes the evidence-bound `adopt`,
   `reject` or `inconclusive` through the existing benefit gate, idempotently.
   Failed attempts stay included, measurement bounds stay separate from
   paired variation, and missing attribution that could change the decision
   stays inconclusive rather than favorable. Missing, contradictory or
   expired evidence cannot authorize adoption.
7. **Settlement.** A settled decision routes to the merged owners without
   another user confirmation: a completed real task is retained identity-only
   as replayable pre-solution inputs under the card that owns it;
   corroboration units are selected when the predeclared policy requires
   them, and too few applicable units stay explicitly inconclusive; an
   unadopted decision reconciles the hypothesis's own change (retention reads
   its actual task state and writes nothing, while archival uses only the
   supported non-synchronizing path and only when every required task is
   done), and no unadopted delta reaches the main specs. Adoption integrates
   only the exact supported, authorized candidate: the combined-tree check
   runs in the candidate's owned worktree, and the exact evaluated revision is
   fast-forwarded only when the newest decision, removal authority, base and
   candidate tree still match; then the prepared candidate runtime is
   activated as the experimental baseline. Benefit, authorization,
   integration, activation and live publication stay distinct — live
   publication follows the installation/skill lifecycle and never follows
   automatically. Rejection, an inconclusive result, drift, a failed check or
   missing removal authority leaves the accepted baseline unchanged while
   candidate, evidence and reason are retained. Restoring an owned
   experimental selection to the unchanged accepted baseline is part of
   authorized recovery, not a new retirement, and never bypasses removal
   approval.
8. **Baseline reuse.** Prepared variants are immutable identities: reuse them
   without source revert, rebuild or model call while their inputs,
   qualification and consumption identity still validate. After an
   interruption, a completed arm may be reused only while its planning inputs
   and qualification remain valid; otherwise the controller records why
   remeasurement is required and no result from the stale arm is paired. An
   active attempt keeps its frozen runtime until it finishes or is explicitly
   cancelled, and unknown restoration blocks conflicting use of that resource
   while independent work continues.
9. **Stop, resume and rotation.** `stop` suspends new work, preserves every
   attempt and marks in-flight attempts unknown so `resume` never replays
   them. `resume` takes over a stopped or interrupted run, reconciles recorded
   receipts, re-resolves the current removal authority and advances only
   settled work; it never replays an unknown or already completed effect. A
   declared successor (`--successor-spec`/`--successor-run`) starts only after
   the completed decision with retained lineage; without provable lineage, or
   when the successor spec merely repeats the completed experiment, it is
   refused, and the controller records a non-suppressing idle continuation
   rather than calling a model to stay busy. An independently specified C can
   follow on the evaluated baseline with its own card and change, but absolute
   durations of different B and C tasks are not compared as a speed trend. The
   installed actual-model A-on-B/B-on-C acceptance remains open until it is
   genuinely exercised.

`status` prints the cursor with no model call: current phase and condition,
the hypothesis card, the qualified planning change, the effective runner
binding, the evidence root, consumed intake outcomes, the selected candidate
with branch/base/revision, the dispatch gate, the removal gate, the
directed-measurement gate state, every attempt with its receipt, the selected
prepared variant, the decision-boundary receipts and the phases still pending.

## Operate and accept

Establish the authorized project, runtime and operating conditions from the
request and existing records. Require observed repeatability through the
actual local agent and tools with an explicit identity policy and comparison
rule declared before the repeats; a fixed seed or temperature alone is
insufficient. API-observed identity records required server/client
observations and unavailable facts as limits; full-material identity
additionally requires those material facts. Missing required observations,
drift or divergent output holds dependent comparisons. Never silently change
the policy, provider or billing route. Every model conversation uses the
visible dispatcher and its own titled terminal surface; a missing surface
suspends new model work instead of a hidden fallback, and idle, blocked or
stopped conditions are reported rather than worked around.

Freeze B's task, specification and source separately from baseline H and
candidate H+A runtimes. Fresh task copies and executor contexts must not
expose sibling solutions through files, Git history or conversation state.
Keep the supervisor and independent oracle outside candidate writes; a
hypothesis that targets evaluation or control components keeps an unchanged
evaluator, and the candidate cannot redefine its own acceptance or activate
itself. Measurements sharing local inference hardware do not overlap with
other loop-generated model work unless the concurrency is an explicitly
controlled part of both arms; report external interference that prevents a
valid comparison.

Declare correctness, meaningful time/resource effects, tolerances, stopping
and selection policy before results. Count failures, retries, checking,
coordination and shared costs once; unknown measurements remain unknown. An
applicable real comparison supports only its measured scope, and an
unmeasured broader claim is not inherited from it. A smaller catalogue or
fewer files alone does not demonstrate efficiency.

Read current removal authority before every affected experiment, integration
or publication, including after resume. Run-start authority and a positive
benefit result do not grant removal consent. Prepare the reviewable proposal,
lost scenarios, alternatives, retained checks and restoration before asking
for the user's scoped decision through `feedback removal-propose`,
`removal-decide` and `removal-check`; one unchanged approval covers the
stages it expressly names (isolated experiment, accepted-source integration,
installed publication). A pending, refused, withdrawn or changed decision
blocks the dependent removal effect while independent authorized work
continues, and a refusal is not re-asked without a new evidential basis.
Investigators and planners may continue; treatment-applying conversations and
candidate selection wait.

On stop or interruption, use the controller's owned recovery. Preserve useful
patches, required evidence and original errors; never replay an unknown model
attempt. Reuse completed work only while its inputs and qualification remain
valid. Report incomplete acceptance explicitly: wiring fixtures and finite
observations cannot prove the full installed loop or indefinite reliability.
