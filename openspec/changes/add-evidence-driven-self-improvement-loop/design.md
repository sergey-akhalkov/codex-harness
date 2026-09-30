## Context

See [proposal.md](proposal.md) for motivation and scope. The confirmed operating scenario is one owner starting a continuous local improvement loop, leaving it running across successive experiments, inspecting decisions through Beads, and stopping or returning without losing useful work. The accepted sequence is A evaluated on B, then B on C. Every hypothesis must have its own complete OpenSpec change before implementation, including small instruction or configuration changes.

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

- Obtain one useful complete A-on-B experiment early, then deliver the whole continuous B-on-C, recovery and installed-operation contract.
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

The skill starts or attaches to an explicitly configured run. The controller performs board operations, qualification checks, dispatch, attempt observation, independent acceptance, accounting and recovery. It asks the configured investigator for a bounded diagnosis/plan only when evidence is available. Researcher and executor conversations use the owning visible dispatch path with concise relevant context; a single ever-growing chat is not the service state.

A run input names the project/board/specification root, base revision, writable scope, model/runner configuration, local evidence root, comparison policy and permitted activation/publication scope. These are explicit local inputs, not checked-in endpoints or machine paths. One controller owns a run and its shared measurement resources. Existing ownership/process facilities prevent a second start from mutating the same run; this does not require a distributed scheduler.

**Alternative:** implement the entire loop as a long skill conversation. Rejected because interruption recovery, measurement boundaries and idempotent board transitions would depend on remembered prose instead of existing executable facilities.

### 2. Beads is the only hypothesis and decision authority

Use one `task` with label `hypothesis` for each hypothesis. Keep its observation, mechanism, scope, predeclared acceptance, OpenSpec reference, exact candidate references, experiment references, decisions and reconsideration conditions on that card. Reuse the existing benefit-gate owner for `adopt`, `reject` and `inconclusive`; evidence locators and experiment identity extend its existing contract as required. A consistent record is not execution proof.

Use ordinary work statuses separately from benefit outcome. An implemented B remains pending its own benefit evaluation. An evidenced rejected experiment can close normally. An inconclusive investigation records the missing fact and is deferred when no immediate justified check remains. Search includes closed and deferred cards. Reconsideration requires new evidence and preserves prior conclusions rather than overwriting them.

Removal proposals and user decisions use this same card and its existing metadata/comment facilities, with references to the reviewed OpenSpec scope and local preview/evidence. Keep consent separate from the benefit-gate result: a successful experiment can await integration approval, and user refusal is not a measurement failure. No new tracker, consent service or hypothesis status hierarchy is needed.

Record the evaluation relationship using the installed nonblocking `related` edge plus the experiment's explicit candidate/workload roles. Do not use `blocks` for A-tested-on-B: deciding A requires accepted B attempts, not B's future benefit decision. Real source dependencies are different and can make a workload ineligible for a particular pair.

Local phase receipts and process identifiers are recovery data, not another task journal. Persist large immutable inputs, patches and traces in the existing local evidence lifecycle and link them from cards. The board alone does not claim an evicted trace remains verifiable. Retention protects active experiments and recovery/baseline provenance, and reports missing historical evidence honestly. Hypothesis cards are durable work items, not ephemeral records subject to automatic purge.

The existing feedback incubator remains lead-owned. The new explicitly invoked improvement controller uses supported `bd` operations for its own cards; it does not take over background incubator sweeping, manufacture diagnostic votes or parse the underlying database. Existing vote thresholds do not gate an already evidenced and planned hypothesis.

**Alternative:** a separate Markdown or JSON hypothesis journal. Rejected because it would duplicate board identity, status and decisions. A custom Beads type is unnecessary unless future observed needs exceed `task` plus a label.

### 3. Each hypothesis has a mandatory, linked OpenSpec change

Before any candidate edit, resolve the intended OpenSpec root/store, scaffold through `openspec new change`, create proposal, requirements, design and tasks using the installed instructions, and validate completeness and content. Include the mechanism, counterexample, baseline/workload identity, independent acceptance, meaningful effect and comparison/stopping policy. This requirement applies to A and to the workload B before either is implemented. It overrides the ordinary optional-spec exception only for this loop's hypotheses.

Beads owns the lifecycle and decisions. OpenSpec owns intended behavior, technical design, acceptance and the implementation checklist. Use references rather than duplicate conclusions. An experiment is specified to support adopt/reject/inconclusive outcomes: rejecting the candidate can finish its agreed investigation, but cannot mark undelivered mandatory product behavior as delivered. Do not remove a rejected hypothesis's specification or erase its evidence. Archive only through the existing workflow when its actual completion conditions hold. Rejected or otherwise unadopted capability deltas must not synchronize into the main specifications; use the supported retention/archive path without spec synchronization, or retain the change with an explicit unresolved archive action if that path is unavailable. Adoption synchronizes only behavior actually accepted and delivered within the declared scope.

Planning and implementation are distinct controller phases and executor assignments. A user-authorized autonomous loop can move between them within its established scope; missing authority or a material outcome change pauses the dependent phase. This proposal itself authorizes no implementation. Protected workflow definitions remain untouched.

### 4. Select ideas from trace evidence and code, then predict a falsifiable effect

The deterministic observation path summarizes measured model/tool intervals, recorded token categories, failures, repeated requests with input revisions and useful acceptance results. It preserves drill-down locators rather than loading complete traces into every researcher context. Existing token-audit findings remain useful sources; missing local-model metrics need an explicit compatible observation path in the existing telemetry owner.

The investigator inspects a bounded set of relevant evidence and source, searches prior cards, and checks official tool capabilities where needed. It must explain why a repeated operation is unnecessary: identical filenames alone do not establish waste if content or available context changed. Rank candidates by observed incidence, attributable cost, strength of mechanism, expected useful effect and implementation/evaluation cost. No invented precision or unconditional numeric score is required.

Seed the first A and B from existing attributable evidence, reproducible friction or observation of authorized real tasks. If no defensible candidate exists, report idle with the next possible evidence source. Continuous availability does not require endless model calls or increasingly speculative changes.

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

For an A-on-B experiment, retain three identities: base harness H, exact candidate runtime H+A, and the frozen task-B source/contract S. Prepare two equivalent owned copies of S with independent homes, caches according to policy, instructions and tools. Install/select each arm's runtime through the existing lifecycle. Prevent inherited live links, project instructions or global configuration from silently replacing the intended arm, and record what was actually consumed.

Keep candidate code on a dedicated hypothesis branch in an owned Git worktree. A branch preserves the implementation and history; the worktree provides a simultaneously usable directory. Reuse an existing allocation only after checking ownership, current branch/base, active use, dirty/untracked files and unmerged commits. Otherwise allocate another owned worktree. Freeze a committed base before dispatch and a committed candidate before measuring; necessary uncommitted inputs must be preserved and explicitly resolved, never silently omitted or copied live between arms. Associate branch/revisions and a local worktree locator with the Beads card. No feature-flag layer is required for every experimental change.

Prepare H and H+A runtime artifacts once and reuse them while source, configuration and build inputs are unchanged. The proposed `improve select --run <id> --variant baseline|candidate` operation selects the prepared variant through the existing runtime-selection owner, returns its effective identity, and performs no model call or source revert. It rejects changes during an active measured attempt; subsequent attempts still begin with fresh executor context. Missing/stale artifacts cause explicit preparation or a clear error, not a silent rebuild hidden from accounting. The controller uses the same selection primitive for paired trials.

Both executors implement B against S. Neither receives an existing B solution, sibling branch/logs, investigator notes that reveal the solution, or mutable shared candidate files. An identifier can bind an artifact but does not prove it executed. The independent oracle observes the real product entry point and remains outside candidate writes. Deliberately wrong implementations must fail that oracle before it is trusted for comparison.

Linked worktrees share repository objects and most references, as documented by [Git worktree](https://git-scm.com/docs/git-worktree). They therefore retain candidates but do not by themselves hide an earlier B solution from another B executor. Materialize each measured source snapshot without sibling solution history, using an independent minimal repository when Git-aware tools require one. Keep shared MCP workers, ports, installation configuration and caches separately allocated or explicitly serialized/controlled. This preserves the convenience of candidate worktrees without claiming they isolate external runtime state.

If B inherently requires A's source change, choose a different applicable real task; do not compare different source prerequisites and call the result a harness effect. An experiment that legitimately changes recurring tool preparation includes it according to the declared everyday warm/cold operating condition rather than hiding it behind asymmetric setup.

**Alternative:** install A in the current session and solve B again. Rejected because remembered solutions, shared files and changed task source would confound the result.

### 6. Advance one candidate at a time and preserve exact lineage

The normal path is:

1. Read Beads and evidence; select A and applicable B; complete both planning contracts and any removal approval required for their experimental implementations.
2. Implement and independently check candidate A on its owned branch/worktree; freeze its commit and prepared runtime identities.
3. Implement B under H and H+A using fresh contexts and the declared paired order/repetitions; check and retain both results.
4. Evaluate A using its predeclared scope, quality and benefit policy; publish an evidence-linked decision.
5. On supported adoption with applicable removal approval, integrate the evaluated candidate into the accepted mainline within the run's authority, check the combined tree and confirm the corresponding H+A runtime before advancing the experimental baseline; otherwise retain H and expose any pending authority. A changed base or resolved conflict requires revalidation of affected benefit evidence and coverage of the actual removal scope. Keep integration/activation receipts distinct from benefit, user authorization and live publication.
6. Select an exact useful B patch, integrate and check it on B's candidate branch against the resulting baseline, and record any changed identity. Another correct B patch is not interchangeable with a tested one; successful workload implementation does not authorize unproven mainline integration or unapproved removal.
7. Generate/select and specify C from current evidence; repeat with B as candidate and C as workload.

Extra reciprocal A/B experiments are optional investigations, not a loop prerequisite. If a candidate is invalid, cannot be rebased without changing its claim, or does not exercise the needed mechanism, preserve it with the explicit next action instead of forcing rotation. If A and B each worked against H, their combination still needs validation against the new baseline.

### 7. Qualify the actual local runtime before cost comparisons

Extend runner selection with an explicit local-provider configuration common to both arms. Do not permit changing provider/model through arbitrary candidate treatment settings. Preserve existing explicitly selected routes, but never silently use them when the requested local model is unavailable. Verify the real endpoint contract, effective model/weights and quantization, tokenizer/template, sampling/seed and reasoning settings, context/KV-cache configuration, tool round-trip, usage coverage and stop behavior when those inputs are supplied.

Repeated calibration uses the actual agent/tool path with identical controlled inputs. Pin clocks, random inputs, file ordering and comparable cache/load state where they enter the task; define any nonsemantic normalization before observing output. Required solution output must repeat. A finite qualification establishes observed repeatability for that configuration, not mathematical determinism of every future execution. Divergence or material unknown configuration blocks dependent strict comparisons and requires correction or an explicit user decision on a different policy.

The intended deployment is a local quantized model with a large context capacity; its exact supplied identity is authoritative. Neither its ability to solve a task nor full-window usefulness is assumed. The published [llama.cpp server contract](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md) illustrates why seed and temperature alone are insufficient: some cache/batch/backend combinations can change logits. It does not select that server for the user.

### 8. Decide on correctness and separate time/resource effects

Keep independent task acceptance as a prerequisite. Report elapsed time through verification and corrections, request/interaction/tool-operation counts, input/output/cache/reasoning categories with subset relationships, and missing coverage. Steps diagnose mechanisms but are not a model-independent price. Token comparisons name the model/tokenizer. Currency, energy or hardware cost require an explicit measured basis; bytes or subscription totals do not supply one.

Report reusable planning, investigation, candidate construction, repeated trials, integration and recovery separately from the matched task attempts. Count each shared activity once. Per-accepted-task resources include unsuccessful attempts and acceptance rate; zero accepted tasks has no finite per-success estimate. The fixed planning artifacts shared by a pair are a common input, with their preparation cost retained as experiment overhead. Claims about a planning-only optimization require a workload that actually exercises that work, not an extrapolation from implementation timings.

Before comparisons, declare a practically meaningful effect and the allowed noncritical measurement variation. The default decision rule preserves correctness and requires a meaningful improvement in time or resources without a material regression in the other. A genuine cost/time trade-off needs an explicit predeclared policy rather than an invented weighted score. Do not mistake lack of detected regression for proven equivalence.

Apply the same rule to subtraction through the [subtractive-treatment contract](specs/harness-outcome-evaluation/spec.md#requirement-subtractive-treatments-prove-useful-effects-and-retained-behavior). Include changed discovery/setup, manual fallback and recovery costs; measure actual context consumption separately from invocation. A proposed maintenance-only benefit without an efficiency effect needs a separately predeclared user-agreed basis, not a post-hoc exemption for fewer lines. Freeze any explicitly approved retirement of product scope before the comparison, keep retained task acceptance common to both arms, and exercise affected consumer and restoration paths independently of the optimizing executor.

Control order, startup, hardware contention and cache policy within matched blocks. Compare the two arms of B, then separately the two arms of C; absolute durations across different tasks do not measure improvement. The experimental-unit principle follows [NIST's blocked-comparison guidance](https://www.itl.nist.gov/div898/handbook/pri/section3/pri332.htm), applied to the actual task and environment rather than an assumed universal sample size.

Preserve replayable real tasks linked to their existing cards. Use applicable earlier tasks, with checks hidden from the optimizing executor as appropriate, when corroboration is required for the declared adoption scope. A first pair can establish a task-scoped result, not universal savings. Predeclare repetitions/stopping and repeated-selection treatment; do not run until the first favorable pair appears. Transfer beyond the measured local model remains unproven until exercised.

### 9. Recover deterministic bookkeeping without replaying unknown effects

Use a small persisted phase cursor referencing board/experiment/artifact identities. Natural phases are planning, candidate-ready, baseline-attempt, candidate-attempt, acceptance, decision-recorded and activation-confirmed, plus explicit idle/blocked/stopped conditions. These are operational receipts, not a second set of hypothesis statuses.

Pending removal approval is a waiting-for-input reason referencing the board decision, not a competing lifecycle. Resume resolves current approval/refusal/withdrawal before another removal effect; stale receipts cannot grant consent. Preserve completed effects and evidence, report any restoration needed, and do not replay the experiment merely to recover an approval reference.

Give an experiment and decision stable identities. Write completed evidence before publishing its decision. On restart, read the board and exact artifact/runtime identities, inspect the known process result, and complete only the missing idempotent action. A decision recorded before activation must not cause an unverified baseline advance or another billed trial. Unknown model/process outcomes require reconciliation, not automatic resubmission.

Stop dispatches no new work and uses the existing owned process lifecycle to settle or identify in-flight effects. Preserve useful patches and partial measurements. A changed baseline, model or environment can make a completed arm unsuitable for reuse; report that explicitly. Endpoint failures use configured bounded backoff and a visible blocked reason. No arbitrary total-duration cap replaces 24/7 operation; budgets constrain the work they actually describe.

Serialize model work on shared local inference hardware during comparisons. Every active model conversation has its own titled visible surface through the actual dispatch mechanism. A missing surface suspends new dispatch. The controller never infers state from window pixels or hides auxiliary model calls.

### 10. Reuse installation and publication boundaries

Experimental advancement operates on isolated runtimes and the agreed source scope. Candidate code enters the accepted mainline only after supported benefit and combined-tree checks; preparation, planning and branch retention do not imply a merge. Live activation is a separately identifiable action using established authority and existing lifecycle checks. Rejection keeps the mainline unchanged, so routine testing and rejection use neither revert commits nor manual code cleanup. Preserve useful rejected/deferred commits and evidence. Reclaim a worktree only after its work is preserved and it has no active consumers, using the normal non-forced Git lifecycle; do not reset another task's tree or delete unmerged work. A candidate that changes the controller or evaluator runs beneath an unchanged external supervisor/oracle and cannot replace its own running control rules.

For approved subtraction, these owners must also verify the intended capability was actually removed from the selected scope, unrelated capabilities remain available and the documented restoration works. Approved source integration does not imply installed retirement unless that stage was expressly covered. Preserve still-needed knowledge, links and evidence in their owners before consolidating or retiring documents and skills.

Deliver the new skill and CLI through the kit's normal installed package, verify actual consumption outside this checkout, and preserve unrelated settings and recovery. Instructions, skills, MCPs, plugins and tools are eligible treatment domains, not exemptions from dependency trust, accepted language ownership or skill-publication requirements. Author reusable capability only in this repository.

## Risks / Trade-offs

- **Self-generated workloads reward narrow specialization** -> retain real task snapshots and applicability, use predeclared corroboration and limit each claim to its measured domain/model.
- **Second attempts inherit the first answer** -> separate ownership of the runtime, source, home and context; test intentional contamination through a sibling solution artifact and shared Git references.
- **A candidate edits its own grader or hides failed work** -> independent read-protected acceptance and parent-owned accounting; exercise forged-success and dropped-attempt cases.
- **Strict repeatability is unsupported by the supplied server** -> keep execution qualification open and name the failing observation; do not silently relax the user's requirement.
- **Research costs exceed savings** -> record complete overhead and the explicit use horizon for net-benefit claims; prioritize strong evidence and permit idle operation.
- **Nonuse hides a rare dependency, or a smaller catalogue only looks cheaper** -> record coverage gaps, check supported indirect/recovery uses, verify actual consumption and require informed removal approval.
- **Consent is mistaken for benefit, or benefit for consent** -> retain distinct decisions on the existing card and verify both before the applicable transition, including resume.
- **The simplification review becomes new overhead** -> reuse bounded evidence and existing owners at intake, with no deletion quota or mandatory recurring inventory.
- **A mandatory spec costs tokens even for a small hypothesis** -> keep artifacts concise and reuse native templates; include that cost rather than introduce a small-change exception to the confirmed requirement.
- **Beads or installation actions partially succeed** -> reconcile recorded decisions and exact activation identities through existing idempotent owners; retain the original failure and recovery evidence.
- **Large transcripts fill storage or leak into shared source** -> bounded local retention protecting active evidence, compact board references, synthetic public examples and native source hygiene checks.

## Migration Plan

1. Add the new controller/CLI and extend existing board/outcome owners with model-free acceptance. Keep existing feedback, runner selection and installation behavior compatible.
2. Deliver one installed end-to-end A-on-B path on real supplied local inference, with independent acceptance and Beads/OpenSpec records. Do not declare the full change complete at this milestone.
3. Complete automatic B-on-C rotation, simplification intake and scoped removal decisions, safe branch/worktree reuse, prepared-variant selection, benefit/authority-gated mainline integration, exact-baseline advancement, retained real-task corroboration and interruption recovery.
4. Deliver and exercise the installed skill/CLI outside the checkout, verify update/recovery and an explicitly bounded continuous-operation observation with multiple experiments and stop/resume. The observation duration is evidence scope, not a service lifetime restriction.
5. Update owning operating guides and retain commands, identities and private evidence locators. Rollback restores the prior installed/experimental selection through the existing lifecycle, keeping hypothesis history and owned evidence intact.

## Open Questions

The local endpoint, exact model artifact, serving backend, hardware and supplied runtime settings arrive later. Their values are run inputs and determine qualification evidence; they do not change the architecture or authorize guessed defaults. Actual task durations, repeatability and metric coverage are unknown until that qualification. Native preparation can proceed, but the model-backed and installed-consumer acceptance tasks must remain open until exercised.
