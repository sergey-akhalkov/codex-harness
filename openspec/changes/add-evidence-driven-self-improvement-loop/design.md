## Context

See [proposal.md](proposal.md) for motivation and scope. The confirmed operating scenario is one owner starting a continuous local improvement loop, leaving it running across successive experiments, inspecting decisions through Beads, and stopping or returning without losing useful work. The confirmed cycle is initial measurement, hypothesis formulation, implementation, remeasurement, a supported decision and verified adoption or restoration, followed autonomously by another grounded hypothesis. Each hypothesis has its own OpenSpec change from targeted initial measurement onward, completed before candidate edits. A-on-B then B-on-C remains supported when justified; it is not the mandatory shape of every experiment.

Relevant existing owners, inspected during planning:

- `crates/harness-core/src/board_cli.rs`, `board_feedback.rs` and `board_lifecycle.rs`, exposed through `crates/codex-harness/src/feedback_cli.rs`, own Beads-backed operations. The installed board supports `task`, labels, metadata, comments, `related` edges and ordinary open/in-progress/deferred/closed states. Its existing benefit-gate owner must be extended instead of creating another ledger.
- `crates/codex-harness/src/outcome_run.rs` runs one explicitly opted-in native attempt with owned case/home roots, process observation and retained evidence. It currently selects fixed model routes; its request is not a general local-model contract. `tests/outcome_run.rs` deliberately rejects provider endpoints passed as arbitrary treatment configuration. Reuse its isolation and observation, preserving that boundary while adding explicit common runner configuration.
- `crates/harness-core/src/outcome_report.rs`, the `outcome_*` CLI modules and tests own outcome accounting and independent checks. Report formatting alone does not execute or establish acceptance.
- `crates/harness-core/src/rollout_reader.rs` is shared by token audit and delegation accounting. Its documented measured/inferred distinction and missing-usage handling remain authoritative. Local-provider coverage requires actual verification, not an assumed compatible wire format.
- Existing visible dispatch, skill packaging, publication and installation/recovery are the delivery path. Skill-library changes still pass through the owned skill-evolution path and applicable skill evaluation; this loop does not become a second skill publisher.
- Existing outcome specifications already require independent acceptance, complete accounting, uncertainty and evidence-bound adoption. This change specializes those rules for continuous real-task experiments.

These are source-inspection findings. Local-runtime qualification and model-backed benefit remain unverified and are explicit implementation acceptance work.

## Goals / Non-Goals

**Goals:**

- Obtain one useful autonomous measurement-to-decision cycle early with the smallest sufficient real workload, then finish short and full experiment paths, optional A-on-B/B-on-C rotation, recovery and installed operation.
- Let machine-owned execution bookkeeping be deterministic while model work supplies bounded diagnosis, planning and implementation.
- Preserve the distinction between a correct implementation, an evidenced benefit, an experimental-baseline activation and a live installation publication.
- Make subtraction a normal candidate route while keeping the user's informed removal decision distinct from measured benefit.

**Non-Goals:**

- A new issue tracker, a synthetic benchmark service, or reciprocal evaluation of every pair.
- Model training, autonomous purchases, unspecified provider fallback or automatic publication outside established authority.
- Editing protected OpenSpec workflows or replacing existing installation, skill-evolution, telemetry and process owners.
- Treating a finite experiment as universal reliability, exact future token usage or general benefit across models.
- Automatic retirement from nonuse, deletion quotas, or another scheduled inventory/approval service.

## Decisions

### 1. Extend the existing Rust harness behind a small skill entry point

Add a proposed `codex-harness improve start|status|select|stop|resume` command and owned `.agents/skills/self-improvement-loop/SKILL.md`. Use a small CLI adapter and a cohesive controller in the existing crates, provisionally `crates/codex-harness/src/improvement_loop_cli.rs` and `crates/harness-core/src/improvement_loop.rs`; split only where existing ownership or verification warrants it. No new framework or external dependency is justified at planning time.

The skill starts or attaches to an explicitly configured run. The controller performs board operations, initial measurement, applicable qualification checks, dispatch, attempt observation, independent acceptance, accounting, authorized adoption/restoration and recovery. It asks the configured investigator for a bounded diagnosis/plan only when evidence is available. Researcher and executor conversations use the owning visible dispatch path with concise relevant context; a single ever-growing chat is not the service state.

A run input names the project/board/specification root, base revision, writable scope, model/runner configuration, local evidence root, comparison policy and permitted activation/publication scope. These are explicit local inputs, not checked-in endpoints or machine paths. One controller owns a run and its shared measurement resources. Existing ownership/process facilities prevent a second start from mutating the same run; this does not require a distributed scheduler.

**Alternative:** implement the entire loop as a long skill conversation. Rejected because interruption recovery, measurement boundaries and idempotent board transitions would depend on remembered prose instead of existing executable facilities.

### 2. Beads is the only hypothesis and decision authority

Use one `task` with label `hypothesis` for each hypothesis. Keep its measurement question, observation, mechanism, scope, selected method and rationale, predeclared acceptance, OpenSpec reference, exact candidate references, experiment references, decisions and reconsideration conditions on that card. Reuse the existing benefit-gate owner for `adopt`, `reject` and `inconclusive`; evidence locators and experiment identity extend its existing contract as required. A consistent record is not execution proof.

Use ordinary work statuses separately from benefit outcome. An implemented B remains pending its own benefit evaluation. An evidenced rejected experiment can close normally. An inconclusive investigation records the missing fact and is deferred when no immediate justified check remains. Search includes closed and deferred cards. Reconsideration requires new evidence and preserves prior conclusions rather than overwriting them.

Removal proposals and user decisions use this same card and its existing metadata/comment facilities, with references to the reviewed OpenSpec scope and local preview/evidence. Keep consent separate from the benefit-gate result: a successful experiment can await integration approval, and user refusal is not a measurement failure. No new tracker, consent service or hypothesis status hierarchy is needed.

When a workload is itself a hypothesis, record the evaluation relationship using the installed nonblocking `related` edge plus the experiment's explicit candidate/workload roles. An ordinary build, search or diagnostic workload belongs to the experiment contract and needs no fabricated hypothesis card or change. Do not use `blocks` for A-tested-on-B: deciding A requires accepted B attempts, not B's future benefit decision. Real source dependencies are different and can make a workload ineligible for a particular pair.

Local phase receipts and process identifiers are recovery data, not another task journal. Persist large immutable inputs, patches and traces in the existing local evidence lifecycle and link them from cards. The board alone does not claim an evicted trace remains verifiable. Retention protects active experiments and recovery/baseline provenance, and reports missing historical evidence honestly. Hypothesis cards are durable work items, not ephemeral records subject to automatic purge.

The existing feedback incubator remains lead-owned. The new explicitly invoked improvement controller uses supported `bd` operations for its own cards; it does not take over background incubator sweeping, manufacture diagnostic votes or parse the underlying database. Existing vote thresholds do not gate an already evidenced and planned hypothesis.

**Alternative:** a separate Markdown or JSON hypothesis journal. Rejected because it would duplicate board identity, status and decisions. A custom Beads type is unnecessary unless future observed needs exceed `task` plus a label.

### 3. One linked OpenSpec change owns each hypothesis from initial measurement

Before the first targeted measurement for an investigation, resolve the intended OpenSpec root/store and scaffold its change through `openspec new change`. Start from existing attributable observations and an explicit measurement question, scope, inputs, baseline-measurement tasks and local evidence references; a concrete solution is not required yet. Routine telemetry and previously retained evidence do not need a new change per event. Use the same change as measurements inform the falsifiable hypothesis, design, candidate implementation, new measurements, decision and adoption/restoration checks.

Before any candidate edit, complete proposal, requirements, design and tasks using the installed instructions, validate them, and freeze the mechanism, counterexample, claimed scope, selected method, baseline/workload identity, independent acceptance, meaningful effect, controls and stopping/escalation policy. This applies separately to A and B when both are improvement hypotheses, including small instruction/configuration changes. A build or other ordinary workload uses its existing contract or the hypothesis's experiment plan, not an artificial B change. A materially different causal hypothesis receives its own change; further trials of the same hypothesis remain linked to its existing owner.

Beads owns the lifecycle and decisions. OpenSpec owns the initial measurement and experiment plan, intended behavior, technical design, acceptance and the measurement-to-decision checklist. Use references rather than duplicate conclusions. An experiment is specified to support adopt/reject/inconclusive outcomes: rejecting the candidate can finish its agreed investigation, but cannot mark undelivered mandatory product behavior as delivered. Do not remove a rejected hypothesis's specification or erase its evidence. Archive only through the existing workflow when its actual completion conditions hold. Rejected or otherwise unadopted capability deltas must not synchronize into the main specifications; use the supported retention/archive path without spec synchronization, or retain the change with an explicit unresolved archive action if that path is unavailable. Adoption synchronizes only behavior actually accepted and delivered within the declared scope.

Planning and implementation are distinct controller phases and executor assignments. A user-authorized autonomous loop can move between them within its established scope; missing authority or a material outcome change pauses the dependent phase. This proposal itself authorizes no implementation. Protected workflow definitions remain untouched.

### 4. Select ideas from trace evidence and code, then predict a falsifiable effect

The deterministic observation path summarizes measured model/tool intervals, recorded token categories, failures, repeated requests with input revisions and useful acceptance results. It preserves drill-down locators rather than loading complete traces into every researcher context. Existing token-audit findings remain useful sources; missing local-model metrics need an explicit compatible observation path in the existing telemetry owner.

The investigator inspects a bounded set of relevant evidence and source, searches prior cards, and checks official tool capabilities where needed. It must explain why a repeated operation is unnecessary: identical filenames alone do not establish waste if content or available context changed. Rank candidates by observed incidence, attributable cost, strength of mechanism, expected useful effect and implementation/evaluation cost. No invented precision or unconditional numeric score is required.

Seed each investigation from existing attributable evidence, reproducible friction or observation of authorized real tasks. Select its workload separately; only an independently grounded hypothesis can later become candidate B. If no defensible candidate exists, report idle with the next possible evidence source. Continuous availability does not require endless model calls or increasingly speculative changes.

#### Choose a sufficient experiment before committing to its cost

Use the existing skill to guide judgment and the existing comparison policy/controller to enforce the selected contract. Extend `crates/harness-core/src/improvement_policy.rs` and the benefit-gate/evidence owners rather than introduce a benchmark service, method registry or second planning format. The current policy already represents objective, meaningful effect, acceptance, stopping, uncertainty and overhead; its extension must bind the experimental unit/method, claim, admissible baseline, applicability rationale and escalation/deferral conditions. Missing or inconsistent required evidence prevents the corresponding effect/decision; prose alone is not proof of execution.

The investigator applies this decision procedure within each hypothesis's change:

1. Establish the observed burden, available baseline evidence, incidence and intended-use horizon; plan only missing targeted measurements.
2. State the causal mechanism, measurable outcome, minimum useful effect, required quality/regression constraints and counterexample. Keep estimates distinct from measured benefits.
3. Trace where the effect and important regressions arise: an operation, agent choices, a complete accepted result, or a repeated-use/recovery sequence. Choose the smallest unit preserving that path and explain why omitted work cannot change the decision.
4. Assess relevant variability/confounders, error consequences, reversibility and total investigation/implementation/evaluation cost. A costly experiment must resolve uncertainty that matters enough to justify it; otherwise defer without claiming failure or adoption. No line-count shortcut, fixed score or mandatory sequence of cheaper trials is needed.
5. Freeze baseline admissibility, inputs/runtime and acceptance identities, controls, metrics, stopping and justified escalation conditions before candidate implementation and outcomes. Reuse an actual retained baseline only when those conditions hold; record unknowns and obtain a fresh control when they could change the result.
6. Execute, independently check, compare and act under that plan. Retain all outcomes. Escalate only for a named decision-changing observation under the declared policy, or defer and proceed to another grounded hypothesis.

| Mechanism and claimed outcome | Sufficient experiment when applicable | Boundary |
| --- | --- | --- |
| Build reuse, process startup or output transformation | Compare the real operation or bounded retained-input replay under both implementations, with relevant integration/failure checks | Operation savings do not establish changed agent behavior or total task productivity |
| Agent search, command selection or diagnosis | A short real-agent task with independently accepted output | A fixed command bypassing the agent's choices is insufficient |
| Planning, implementation strategy, delegation or corrections | Whole-task paired implementation through accepted completion when shorter work loses those interactions | Fresh contexts, comparable inputs and independent acceptance remain required |
| Recurring cache, long-session or recovery behavior | The relevant sequence and state transitions under both variants | Preserve preparation, invalidation, return/recovery and the declared use horizon |

An operation comparison may itself be A/B; two code implementations are only one possible workload. Replayed tool outputs can validate a local transformation but cannot reconstruct a changed adaptive agent trajectory. A smaller experiment never waives required product behavior, diagnostic checks or a broader agreed outcome. Preliminary measurements inform the plan; success criteria cannot be chosen after candidate results. Ordinary before/after monitoring can motivate a hypothesis but is not a causal comparison when workload or conditions changed.

This adapts [IHI's Model for Improvement](https://www.ihi.org/library/model-for-improvement) for iterative small tests, [NIST's objective-led experiment selection](https://www.itl.nist.gov/div898/handbook/pri/section3/pri31.htm) and [Microsoft's pre-experiment hypothesis and metric guidance](https://www.microsoft.com/en-us/research/?p=680556). Their general principles support the approach; production traffic scale and statistical assumptions are not imported into a single-owner local loop.

**Alternatives:** mandatory full implementation for every change wastes work on local mechanisms; universally replacing real work with microbenchmarks can miss agent decisions and displaced costs. A compulsory ladder adds probes even when the need for a full trial is already evident. Select the sufficient method directly and retain escalation only for unresolved relevant evidence.

#### Apply Occam's razor to candidate selection

Compare the smallest sufficient existing route and no change with addition, simplification and subtraction. The investigator follows the [simplification requirement](specs/self-improvement-loop/spec.md#requirement-simplification-is-a-first-class-improvement-hypothesis), reusing available usage/outcome analysis at ordinary intake boundaries. It does not commission exhaustive audits on every iteration. Rank by attributable burden and expected accepted-result benefit after investigation, migration and recurring costs; line count is a description, not a benefit score.

The following are search directions, not findings that these components are currently unnecessary:

| Candidate area | Evidence worth investigating | What must survive or be explicitly retired |
| --- | --- | --- |
| Skills and tool exposure | Unused/overlapping skills, catalogue or initialization cost; compare narrower exposure or supported on-demand loading | Explicit invocation, rare tasks, indirect use, discoverability and actual context refresh |
| Instructions, templates and documentation | Repeated/conflicting rules, oversized injected text, obsolete copies | One authoritative contract, required instructions and working routes to operational/recovery information |
| Code, features and compatibility branches | Duplicate paths, obsolete switches/adapters, configuration combinations with attributable upkeep or runtime cost | Public/dynamic callers, supported versions, persisted data and migration/recovery |
| Wrappers, abstractions, dependencies, MCPs and plugins | Extra process/translation/setup work without observed value over an existing route | Real consumers, diagnostic detail, dependency trust and installation ownership |
| Workflow stages and delegation | Repeated handoffs, empty workers, duplicated investigation or approvals, polling without a decision-changing observation | Independent acceptance, required conversation visibility and actual concurrency needs |
| Tests, checks and retries | Duplicate checks on unchanged inputs, repeated full runs or retries without new evidence | Distinct failure coverage, required gates, timing/integration conditions and failure visibility |
| Configuration, flags and caches | Stale defaults, duplicate state, costly invalidation or rebuilding with little measured reuse | Supported overrides, effective runtime identity and recovery under cold/changed inputs |
| Logs, reports and generated artifacts | Duplicated records, oversized success output, repeated loading or regeneration | Necessary diagnostics, active evidence, provenance, retention obligations and rollback inputs |

Review records the observed interval and task/environment mix, telemetry gaps, known consumers and a realistic counterexample where the benefit disappears. An unused recovery command may still be essential; a dormant skill may still cost context; a smaller interface may move recurring work to the user. Investigate the relevant case instead of equating nonuse with waste. Confirm the selected runtime actually stops consuming the removed material, using fresh measured contexts where required. Reuse the skill usage/evolution owners for skills and the corresponding lifecycle owners for other domains.

#### Prepare an informed decision before applying removal

The user-confirmed boundary is explicit approval before removing code, features or skills. The [removal requirement](specs/self-improvement-loop/spec.md#requirement-removal-requires-informed-and-scoped-user-approval) also covers experimental deletion and disabling/consolidation that withdraws capability. Investigation can prepare an unapplied diff, target/source references, evidence with measured/predicted effects, lost scenarios, caller/configuration/installation impact, alternatives, checks and restoration instructions. Material unknown consumers remain visible in that proposal. Prefer retaining or narrowing exposure when evidence does not support complete retirement.

Record the explicit user decision on the existing Beads owner and bind it to that proposal's targets, behavior loss and action scope. Before dispatch or mutation, check the scope for both A and workload B. Approval may cover isolated experiments and later integration/publication together; later stages still need benefit and integration checks. Experiment-only approval cannot authorize merging or live retirement. A target/consequence change requires a revised decision; an unchanged covered continuation reuses the approval. A missing or declined decision leaves that candidate pending/deferred while other independent authorized work can proceed.

The controller checks this authority again at integration, baseline activation and publication, and after resume, using the latest board decision. It does not invent a second approval CLI or infer consent from an evaluator verdict. Mandatory per-hypothesis OpenSpec, independent acceptance, accounting, visibility and recovery remain binding; the candidate cannot delete its own safeguards. Retention cleanup of reproducible owned runtime data remains under the existing retention contract and cannot be repurposed to retire capability or discard active evidence.

**Alternatives:** auto-delete after an inactivity threshold, or ask only after applying removal in a worktree. Both fail the informed-before-removal boundary; incomplete usage also makes the first unreliable. Requiring a new approval at every stage despite unchanged explicit coverage adds avoidable user work, so reuse scoped consent instead.

### 5. Separate the executing harness from the source being edited

For each selected experiment, retain three identities: base harness H, exact candidate runtime H+A, and the frozen workload inputs/contract S. In a full A-on-B experiment, S is the task-B source and specification. For fresh paired attempts, prepare equivalent owned copies of S with method-appropriate isolated state, caches, instructions and tools; retain admissible historical baseline evidence instead of creating an unnecessary fresh baseline attempt. Agent trials additionally need independent homes and contexts. Install/select each arm's runtime through the existing lifecycle. Prevent inherited live links, project instructions or global configuration from silently replacing the intended arm, and record what was actually consumed.

Keep candidate code on a dedicated hypothesis branch in an owned Git worktree. A branch preserves the implementation and history; the worktree provides a simultaneously usable directory. Reuse an existing allocation only after checking ownership, current branch/base, active use, dirty/untracked files and unmerged commits. Otherwise allocate another owned worktree. Freeze a committed base before dispatch and a committed candidate before measuring; necessary uncommitted inputs must be preserved and explicitly resolved, never silently omitted or copied live between arms. Associate branch/revisions and a local worktree locator with the Beads card. No feature-flag layer is required for every experimental change.

Prepare H and H+A runtime artifacts once and reuse them while source, configuration and build inputs are unchanged. The proposed `improve select --run <id> --variant baseline|candidate` operation selects the prepared variant through the existing runtime-selection owner, returns its effective identity, and performs no model call or source revert. It rejects changes during an active measured attempt; subsequent agent attempts still begin with fresh executor context. Missing/stale artifacts cause explicit preparation or a clear error, not a silent rebuild hidden from accounting. The controller uses the same selection primitive for paired trials.

Each variant performs the selected workload against S. If that workload is implementing B, both executors implement B independently; neither receives an existing B solution, sibling branch/logs, investigator notes that reveal the solution, or mutable shared candidate files. Direct operation trials need no artificial model conversation; agent-dependent trials still need fresh contexts and applicable qualification. An identifier can bind an artifact but does not prove it executed. The independent oracle observes the relevant real product entry point and remains outside candidate writes. Deliberately incorrect, skipped or forged-success results must fail that oracle before it is trusted for comparison.

Linked worktrees share repository objects and most references, as documented by [Git worktree](https://git-scm.com/docs/git-worktree). They therefore retain candidates but do not by themselves hide an earlier B solution from another B executor. Materialize each measured source snapshot without sibling solution history, using an independent minimal repository when Git-aware tools require one. Keep shared MCP workers, ports, installation configuration and caches separately allocated or explicitly serialized/controlled. This preserves the convenience of candidate worktrees without claiming they isolate external runtime state.

If B inherently requires A's source change, choose a different applicable real task; do not compare different source prerequisites and call the result a harness effect. An experiment that legitimately changes recurring tool preparation includes it according to the declared everyday warm/cold operating condition rather than hiding it behind asymmetric setup.

**Alternative:** install A in the current session and solve B again. Rejected because remembered solutions, shared files and changed task source would confound the result.

### 6. Advance one candidate at a time and preserve exact lineage

The normal path is:

1. Search Beads and evidence, select a grounded investigation, and establish its change and targeted initial-measurement plan.
2. Execute or verify reusable baseline measurements; formulate the falsifiable hypothesis and choose its sufficient experiment. Complete and validate the change before candidate edits, including any required removal approval.
3. Implement and independently check the candidate on its owned branch/worktree; freeze its commit and prepared runtime identity.
4. Execute the chosen baseline/candidate workload comparison, reusing admissible baseline evidence when justified. Preserve independent acceptance, original errors, all attempts and overhead.
5. Evaluate the declared quality, benefit and scope policy and publish an evidence-linked adopt/reject/inconclusive decision.
6. On supported adoption with applicable authority, integrate the evaluated candidate, check the combined tree and confirm the exact runtime before advancing the experimental baseline. A changed base or resolved conflict requires revalidation of affected benefit evidence. Live publication remains separately scoped.
7. On rejection or deferral, restore and verify H at a safe boundary, preserving the candidate patch, initial/new measurements and reason. A selected candidate is removed from active use, not erased from history. Unknown or failed restoration stops conflicting use until reconciled.
8. Select the next grounded hypothesis automatically within run authority, or expose an evidence-based idle/waiting reason. An inconclusive result permits only its justified declared next measurement or deferral, not an endless identical retry.

When useful, the chosen workload can be another improvement B. Retain both accepted B implementations, decide A without waiting for B's benefit, and select an exact B patch only if B is independently chosen next. Integrate/check that patch on its candidate branch against the resulting baseline, recording any changed identity, then evaluate B on an applicable C. This optional rotation preserves useful work and exact lineage without constraining all experiments to produce a next candidate. B's correct implementation is not proof of B's benefit or authority for its adoption/removal.

Routine restoration of the owned experimental runtime to the accepted baseline is covered by the experiment's recovery scope. It does not introduce another removal-approval ritual. Removing accepted capability as a candidate treatment, changing unrelated work, deleting evidence or publishing outside scope still requires the existing authority. Keep decision, integration, activation and restoration receipts distinct.

**Alternative:** choose the next candidate solely because the preceding experiment happened to produce its patch. Rejected because workload applicability and hypothesis value are independent. Optional A-on-B/B-on-C acceptance remains required to verify that supported route.

### 7. Qualify the actual local runtime before model-dependent comparisons

For experiments that execute the model, extend runner selection with an explicit local-provider configuration common to both arms. Do not permit changing provider/model through arbitrary candidate treatment settings. Preserve existing explicitly selected routes, but never silently use them when the requested local model is unavailable. Verify the real endpoint contract, reported model and deployment metadata, template, available sampling/seed and reasoning settings, context/cache observations, client profile/catalogue, tool round-trip, usage coverage and stop behavior. Keep exact deployment inputs private.

Repeated calibration uses the actual agent/tool path with identical controlled inputs. Pin clocks, random inputs, file ordering and comparable cache/load state where they enter the task; define any nonsemantic normalization before observing output. Required solution output must repeat. A finite qualification establishes observed repeatability for that configuration, not mathematical determinism of every future execution.

The user selected API-observed identity for this loop: declare the required observable server and client fields before qualification, retain their actual values and provenance, and compare that same set before dependent attempts. Endpoint/model and effective client configuration remain required. Record unavailable weight hashes and hardware details explicitly without inventing values or blocking this policy solely for their absence. An observation failure, disappearance of a previously required field, changed observed configuration or divergent required output blocks dependent comparisons and requires requalification. Preserve full-material identity checks for runs selecting that stronger policy; the policy itself is part of the qualification identity and never silently changes. API-observed evidence cannot certify unchanged unobservable weights or hardware.

The intended deployment is a local quantized model with a large context capacity; its exact supplied identity is authoritative. Neither its ability to solve a task nor full-window usefulness is assumed. The published [llama.cpp server contract](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md) illustrates why seed and temperature alone are insufficient: some cache/batch/backend combinations can change logits. It does not select that server for the user.

### 8. Decide on correctness and separate time/resource effects

Keep independent task acceptance as a prerequisite. Report elapsed time through verification and corrections, request/interaction/tool-operation counts, input/output/cache/reasoning categories with subset relationships, and missing coverage. Steps diagnose mechanisms but are not a model-independent price. Token comparisons name the model/tokenizer. Currency, energy or hardware cost require an explicit measured basis; bytes or subscription totals do not supply one.

Report reusable planning, investigation, candidate construction, repeated trials, integration and recovery separately from the matched task attempts. Count each shared activity once. Per-accepted-task resources include unsuccessful attempts and acceptance rate; zero accepted tasks has no finite per-success estimate. The fixed planning artifacts shared by a pair are a common input, with their preparation cost retained as experiment overhead. Claims about a planning-only optimization require a workload that actually exercises that work, not an extrapolation from implementation timings. A model-free operation reports model metrics as inapplicable, not measured zero, and does not trigger unrelated model qualification. Measurement scope and claims remain bound to its selected method.

Before comparisons, declare a practically meaningful effect and the allowed noncritical measurement variation. The default decision rule preserves correctness and requires a meaningful improvement in time or resources without a material regression in the other. A genuine cost/time trade-off needs an explicit predeclared policy rather than an invented weighted score. Do not mistake lack of detected regression for proven equivalence.

Apply the same rule to subtraction through the [subtractive-treatment contract](specs/harness-outcome-evaluation/spec.md#requirement-subtractive-treatments-prove-useful-effects-and-retained-behavior). Include changed discovery/setup, manual fallback and recovery costs; measure actual context consumption separately from invocation. A proposed maintenance-only benefit without an efficiency effect needs a separately predeclared user-agreed basis, not a post-hoc exemption for fewer lines. Freeze any explicitly approved retirement of product scope before the comparison, keep retained task acceptance common to both arms, and exercise affected consumer and restoration paths independently of the optimizing executor.

Control order, startup, hardware contention and cache policy within matched blocks. Compare the two arms of B, then separately the two arms of C; absolute durations across different tasks do not measure improvement. The experimental-unit principle follows [NIST's blocked-comparison guidance](https://www.itl.nist.gov/div898/handbook/pri/section3/pri332.htm), applied to the actual task and environment rather than an assumed universal sample size.

Preserve replayable real tasks linked to their existing cards. Use applicable earlier tasks, with checks hidden from the optimizing executor as appropriate, when corroboration is required for the declared adoption scope. A first pair can establish a task-scoped result, not universal savings. Predeclare repetitions/stopping and repeated-selection treatment; do not run until the first favorable pair appears. Transfer beyond the measured local model remains unproven until exercised.

#### Control nuisance factors while preserving paired comparison

The confirmed comparison uses the old and new harness on the same frozen
workload appropriate to the hypothesis, with admissible baseline reuse when
verified. A short operation can establish its declared local effect; counters,
component benchmarks and different-task timings cannot establish unexercised
agent behavior or replace a required full-task comparison. Extend the existing
comparison policy and preflight receipts with the nuisance-control plan before
either arm starts; reuse it unchanged on resume.

Record the task/runtime/acceptance identities, selected model observations,
owned cache snapshots or preparation recipe, cold/warm mode, permitted shared
resources and ordering rule. Check the resulting state, rather than treating a
configured reset or warm-up command as proof. Reset only owned state. Do not
assume an OS, shared compiler or inference cache was reset when it was not
observed. Keep each arm's own cache evolution and model decisions as outcomes.
Required input or qualification mismatches still fail their existing gates;
uncertainty bounds apply only to residual variation allowed by the plan.
For a caching or recovery treatment, equivalent starting conditions must not
erase the mechanism under test. Preparation and warm-up costs remain visible
under the declared one-time or recurring cost basis.

Choose and record arm order before outcomes, using randomized order or a
balanced schedule across declared repetitions. A single pair cannot balance
both orders, so retain its exposure to time/order drift. Keep non-treatment
load comparable where practical; record uncontrolled conditions and their
consequences without multiplying elapsed time by CPU/GPU-utilization factors.
The existing API-observed identity boundary remains unchanged: unavailable
hardware or weight identity stays a disclosed limitation, not a new access
prerequisite.

Classify faults by their observed effect. A verified transport-only idle wait
may qualify for the existing blocking adjustment; ordinary inference/tool
execution or an unattributed request duration does not. A lost response,
retry, changed context or different execution path
cannot be repaired by subtracting its wall duration. Preserve the original
attempt and usage, apply the frozen retry/stopping rule, and withhold a causal
claim when the remaining effect can change its verdict. Treatments of network
recovery or scheduling retain those effects under controlled relevant faults
or load. Required solution-repeatability qualification remains a separate gate
for model-dependent comparisons.

#### Infrastructure waits and causal interpretation

The ordinary case is an agent waiting for the shared `heavy` build slot while
unrelated work occupies it. A longer queue does not establish worse agent work.
Keep two views over the same attempts: observed time/cost through acceptance,
including all waits and failures, and work metrics adjusted only for evidenced
external blocking. Neither view replaces the other. State before the pair
whether the claim concerns work efficiency or operational resource scheduling;
do not choose the more favorable view after seeing results. Work-efficiency
claims use the adjusted view with attribution coverage; operational claims
about queueing require controlled relevant load and retain that waiting as the
effect under test. Unknown currency or compute cost remains unknown.

Extend the existing heavy-command/resource-admission and process/rollout
owners, not a separate monitoring service. Admission evidence must identify
the attempt, tool call, command and admission instance, parent/inherited
admission, resource, verified owner and monotonic wait boundaries ending in
grant, cancellation, timeout or failure. Correlate task activity and model
request/usage identities through the existing evidence owner. Distinguish
unrelated holders, work from this attempt or experiment, and unknown ownership
without importing foreign command contents or machine paths into public
records. The current waiting diagnostic and holder records do not by
themselves provide this complete trace; missing historical boundaries cannot
be reconstructed as measured durations from a final log line.

For an attempt interval `A`, form `Q` from the union of verified externally
blocked admission intervals clipped to `A`. Intersect `Q` with intervals `B`
where the task lifecycle proves that progress awaited those admissions, then
remove intervals `P` of useful concurrent activity from the same attempt.
The deductible duration is `measure((Q intersect B) minus P)` and adjusted
elapsed time is observed elapsed minus that duration. Use interval union, not
a sum of tool durations; inherited/nested admission and shared work are
counted once. Correlation across processes needs a verified common clock
domain or bounded mapping. When a mapping relies on wall time, a clock jump
or unmapped timestamp degrades coverage. A queued
tool alone does not prove that the agent was blocked. Model activity without
a verified wait-only classification counts as possible useful activity, not
as idle time. No negative result or deduction beyond the observed interval
is permitted. For example, a 30-minute attempt with a 10-minute external wait
and 4 minutes of useful work overlapping that wait has 6 deductible minutes
and a 24-minute adjusted duration, while its observed duration stays 30.
This is a defined normalization of observed work, not a prediction of the
completion time on a hypothetical unloaded machine. A fully observed immediate
grant proves zero queue delay; absent queue telemetry does not.

Implement the adjustment as deterministic reduction of retained native events
under the frozen policy. Correlate and validate identities and boundaries,
classify each interval/request as eligible external waiting, attributable work
or unresolved, then perform the interval operations above. Keep the evidence
reference and rule/reason for every exclusion and unresolved classification.
Replaying the same complete evidence and policy must reproduce the same
deductions and decision inputs without a model call or a manual per-run label.
For each measured quantity eligible for subtraction, reconcile observed total
as adjusted total plus excluded amount; unresolved usage remains included.
Do not apply a time deduction proportionally to tokens, energy or currency.
Reject inconsistent totals or duplicate request usage as accounting gaps.

For the delivered Windows path, reuse the existing QPC sample owner for
controller and admission events in the same verified host/boot clock domain.
This avoids converting those events through wall time; wall-only, foreign or
older-clock-domain evidence still needs a verified mapping. Record the queried
frequency and observed boundary brackets. QPC resolution does not bound event
delivery or scheduling delay, and cross-thread ordering retains one-tick
uncertainty. This choice follows [Microsoft's timing guidance](https://learn.microsoft.com/en-us/windows/win32/sysinfo/acquiring-high-resolution-time-stamps)
without requiring another timing service or hardware-specific clock reader.

The existing raw executor detail stream is bounded and can fill before a
substantial task finishes. Retain the compact facts needed for measurement in
the existing observation lifecycle and expose overflow or gaps explicitly;
do not infer inactivity from a full detail file or require unlimited payload
retention. Usage-record timestamps alone do not establish request boundaries.

Report blocked duration, queue exposure, polling requests/operations and
waiting-related usage separately. Passive waiting consumes no model tokens
by itself. Subtract token usage only for whole requests independently linked
to that admission, contained in the evidenced blocking episode and structurally
established as wait-only; mixed task/status requests and requests crossing an
unresolved boundary remain included and their attribution is incomplete.
Do not infer token quantities from seconds, response length or an agent's
description. Preserve input/cache/output/reasoning subset relationships.
Tool polling without a model request has operations but no invented model
usage. Candidate-induced polling frequency, extra calls, self-contention,
redundant builds, execution time after admission and delay after a slot is
granted remain attributable work/operating costs. Removing verified wait-only
usage from the adjusted view never hides those categories or establishes a
net-cost gain. A treatment that reduces build demand keeps the reduced build
work in adjusted metrics and its changed queue exposure in the operational
view. A scheduler or waiting-policy treatment cannot normalize away its own
declared mechanism.

Even an external queue can cause downstream context growth, compaction,
deadline expiry, retries or a changed plan. Removing its idle interval or a
whole wait-only request does not remove those consequences. Retain later
mixed-request usage, including repeated waiting context, rather than guessing
what part of its input or reasoning would disappear. Expose such effects and
apply the material-uncertainty gate to the causal conclusion. This does not
require identical tool sequences across arms: strategy changes caused by the
treatment remain outcomes.

Freeze the attribution rule/version, eligible causes, claimed metric view,
coverage requirements and material-uncertainty rule with the comparison
policy. Apply them identically to both arms. Missing boundaries, unknown
holder identity, mixed-request usage or slowdown while actually executing
under contention do not authorize guessed subtraction. Keep the verified
deduction and unresolved portion visible; unless sufficient comparable
coverage or a conservative bound proves the decision unaffected, return
`inconclusive`. Queue timeout remains a failed infrastructure attempt, not an
incorrect solution attributed to the model. Retrying requires the existing
predeclared stopping rule; preserve original failures and costs. Replay under
controlled load is a targeted next check, not automatic repetition until a
favorable result. Reports and Beads decisions reference the same raw and
adjusted evidence, exclusions, uncertainty and policy identity.

#### Separate uncertainty sources and validate the measurement path

Keep measurement/attribution bounds separate from empirical variation between
complete paired attempts. Record a bound's evidence and assumptions; do not
assume missing components are zero or independent. Evaluate every decision
gate, including quality, resource regressions and declared trade-offs, over
the supported range. For example, with a 10% meaningful-effect threshold, a
12-18% supported measurement range clears that threshold, while a -3-18%
range does not. Neither range alone establishes statistical confidence about
future agent runs.

Use complete paired task attempts as the units for estimating run-to-run
variation; requests, tools and tokens within one attempt are dependent
observations. A statistical claim needs a predeclared analysis method,
confidence level, assumptions and adequate repetitions for that claim. A
single pair may report its observed task-scoped difference and measurement
bounds, but cannot establish zero model variability or universal savings.
Seed settings and successful qualification do not supply a timing variance
estimate. Apply the existing stopping and repeated-selection policy; when the
allowed evidence is insufficient, retain an inconclusive result rather than
repeat until a favorable result or impose an invented universal sample count.
For evaluating uncertainty, NIST describes [statistical methods](https://physics.nist.gov/cuu/Uncertainty/typea.html)
and [methods using other evidence and assumptions](https://physics.nist.gov/cuu/Uncertainty/typeb.html).
Those classify evaluation methods; they do not make measurement uncertainty
and run variability synonymous with Type B and Type A respectively.

Exercise the same controller/report/decision path with owned independent
controls: identical runtimes with a known external idle delay must retain
the raw delay and neutralize only its evidenced eligible portion; extra build
work, polling, cache benefits and quality failures must remain visible.
Missing boundaries, truncated detail, wrong ownership or unaligned clocks
must widen uncertainty or withhold the conclusion. Retain calibration version,
inputs and results with the existing evidence owner; reuse them only while
the relevant collector, accounting, policy and clock contracts remain valid.
Model-free controls prove the measurement path, not real-model repeatability
or the required A-on-B/B-on-C acceptance.

### 9. Recover deterministic bookkeeping without replaying unknown effects

Use a small persisted phase cursor referencing board/experiment/artifact identities. Natural phases are measurement-planned, initial-measurement, hypothesis/experiment-planned, candidate-ready, baseline-attempt or baseline-reused, candidate-attempt, acceptance, decision-recorded and activation/restoration-confirmed, plus explicit idle/blocked/stopped conditions. These phases extend the existing cursor; they do not require another workflow engine. These are operational receipts, not a second set of hypothesis statuses.

Pending removal approval is a waiting-for-input reason referencing the board decision, not a competing lifecycle. Resume resolves current approval/refusal/withdrawal before another removal effect; stale receipts cannot grant consent. Preserve completed effects and evidence, report any restoration needed, and do not replay the experiment merely to recover an approval reference.

Give an experiment and decision stable identities. Write completed evidence before publishing its decision. On restart, read the board and exact artifact/runtime identities, inspect the known process result, and complete only the missing idempotent action. A decision recorded before activation must not cause an unverified baseline advance or another billed trial. Unknown model/process outcomes require reconciliation, not automatic resubmission.

Stop dispatches no new work and uses the existing owned process lifecycle to settle or identify in-flight effects. Preserve useful patches and partial measurements. A changed baseline, model or environment can make a completed arm unsuitable for reuse; report that explicitly. Endpoint failures use configured bounded backoff and a visible blocked reason. No arbitrary total-duration cap replaces 24/7 operation; budgets constrain the work they actually describe.

Serialize model work on shared local inference hardware during comparisons. Every active model conversation has its own titled visible surface through the actual dispatch mechanism. A missing surface suspends new dispatch. The controller never infers state from window pixels or hides auxiliary model calls.

### 10. Reuse installation and publication boundaries

Experimental advancement operates on isolated runtimes and the agreed source scope. Candidate code enters the accepted mainline only after supported benefit and combined-tree checks; preparation, planning and branch retention do not imply a merge. Live activation is a separately identifiable action using established authority and existing lifecycle checks. Rejection keeps the mainline unchanged and verifies the accepted runtime is restored, so routine testing and rejection use neither revert commits nor manual code cleanup. Preserve useful rejected/deferred commits and evidence. Reclaim a worktree only after its work is preserved and it has no active consumers, using the normal non-forced Git lifecycle; do not reset another task's tree or delete unmerged work. A candidate that changes the controller or evaluator runs beneath an unchanged external supervisor/oracle and cannot replace its own running control rules.

For approved subtraction, these owners must also verify the intended capability was actually removed from the selected scope, unrelated capabilities remain available and the documented restoration works. Approved source integration does not imply installed retirement unless that stage was expressly covered. Preserve still-needed knowledge, links and evidence in their owners before consolidating or retiring documents and skills.

Deliver the new skill and CLI through the kit's normal installed package, verify actual consumption outside this checkout, and preserve unrelated settings and recovery. Instructions, skills, MCPs, plugins and tools are eligible treatment domains, not exemptions from dependency trust, accepted language ownership or skill-publication requirements. Author reusable capability only in this repository.

## Risks / Trade-offs

- **Self-generated workloads reward narrow specialization** -> retain real task snapshots and applicability, use predeclared corroboration and limit each claim to its measured domain/model.
- **Second attempts inherit the first answer** -> separate ownership of the runtime, source, home and context; test intentional contamination through a sibling solution artifact and shared Git references.
- **A candidate edits its own grader or hides failed work** -> independent read-protected acceptance and parent-owned accounting; exercise forged-success and dropped-attempt cases.
- **Required output repeatability or the selected identity policy is unsupported** -> keep qualification open and name the failing observation; do not silently relax the user's policy or turn unavailable facts into verified identity.
- **Research costs exceed savings** -> select the smallest sufficient experiment, reuse applicable baseline evidence, record complete overhead and the explicit use horizon, and defer unjustified measurement without weakening acceptance.
- **Nonuse hides a rare dependency, or a smaller catalogue only looks cheaper** -> record coverage gaps, check supported indirect/recovery uses, verify actual consumption and require informed removal approval.
- **Consent is mistaken for benefit, or benefit for consent** -> retain distinct decisions on the existing card and verify both before the applicable transition, including resume.
- **The simplification review becomes new overhead** -> reuse bounded evidence and existing owners at intake, with no deletion quota or mandatory recurring inventory.
- **A short probe omits the claimed effect, or historical drift looks like a gain** -> bind mechanism, claim, experimental unit and baseline admissibility before results; require a fresh control or a stronger real-agent workload when the missing evidence can change the decision.

- **A mandatory spec costs tokens even for a small hypothesis** -> keep artifacts concise and reuse native templates; include that cost rather than introduce a small-change exception to the confirmed requirement.
- **Beads or installation actions partially succeed** -> reconcile recorded decisions and exact activation identities through existing idempotent owners; retain the original failure and recovery evidence.
- **Large transcripts fill storage or leak into shared source** -> bounded local retention protecting active evidence, compact board references, synthetic public examples and native source hygiene checks.

## Migration Plan

1. Add the new controller/CLI and extend existing board/outcome owners with model-free acceptance. Keep existing feedback, runner selection and installation behavior compatible.
2. Deliver one useful installed autonomous initial-measurement-to-decision path using a sufficient real workload, with independent acceptance and Beads/OpenSpec records. Then exercise the actual supplied local-model A-on-B path; neither milestone alone completes the change.
3. Complete automatic next-hypothesis selection including the supported B-on-C rotation, method selection and baseline reuse, simplification intake and scoped removal decisions, safe branch/worktree reuse, prepared-variant selection, benefit/authority-gated mainline integration, verified rejection restoration, retained corroboration and interruption recovery.
4. Deliver and exercise the installed skill/CLI outside the checkout, verify update/recovery and an explicitly bounded continuous-operation observation with multiple experiments and stop/resume. The observation duration is evidence scope, not a service lifetime restriction.
5. Update owning operating guides and retain commands, identities and private evidence locators. Rollback restores the prior installed/experimental selection through the existing lifecycle, keeping hypothesis history and owned evidence intact.

## Open Questions

The supplied local endpoint and runtime settings remain private run inputs. The selected API-observed qualification records available deployment/configuration facts and the limits of unavailable weight and hardware identity without guessed defaults. Actual task durations, repeatability and metric coverage remain unproven until exercised through the real path. Native preparation can proceed, but model-backed and installed-consumer acceptance tasks stay open until that execution is verified.
