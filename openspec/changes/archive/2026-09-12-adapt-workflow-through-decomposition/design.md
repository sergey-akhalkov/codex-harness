## Context

See [proposal.md](proposal.md) for motivation and the two delta specifications for the behavioral contract.

The current pack already has most execution mechanisms needed here:

- [Portable principles](../../../global/principles-of-work.md) require early useful results, targeted checks, evidence-based retries and proportionate delegation. They do not explicitly recognize a sequence of different failures as evidence that the method of acquiring knowledge needs decomposition.
- [Project verification](../../../.agents/skills/project-verification/SKILL.md) establishes execution identity and a focused-to-required validation path. [Regression reproduction](../../../.agents/skills/reproduce-regression/SKILL.md) already requires failure-preserving reduction for its narrower CLI/MCP/process scope.
- [Token workflow](../../../.agents/skills/token-efficient-workflow/SKILL.md) covers reuse of unchanged evidence and full-cost reasoning. [Worktree isolation](../../../.agents/skills/isolated-worktree/SKILL.md) already distinguishes checkout isolation from shared runtime resources.
- [Delegation guidance](../../../docs/agent-delegation.md) uses ordinary named agents, bounded concurrency and total coordination cost. The existing native probes can run outside the checkout; their current scenarios do not establish adaptive behavior on the cases below.
- [Global instruction delivery](../../../docs/global-instructions.md) uses live source links read by new sessions. [Project decisions](../../../docs/project-decisions.md#openspec-and-completion) exclude edits to externally maintained OpenSpec workflows.

These mechanisms and their current limits were inspected during planning. This is a cross-cutting behavioral change, so a design is needed despite no planned product runtime or dependency addition. The exploration established the problem pattern; it did not measure the proposed policy's effectiveness.

## Goals / Non-Goals

**Goals:**

- Give an agent an observable reason to change its method while preserving progress toward the complete task.
- Make a useful independent result the boundary of decomposition, with inexpensive local feedback and explicit integration.
- Improve both the next feedback cycle and recurring work through reused, project-owned mechanisms when their full cost is justified.
- Validate instruction delivery separately from actual task behavior and measured benefit.

**Non-Goals:**

- A scheduler, persistent reflection daemon, compulsory delegation card or fixed attempt/time quota.
- A universal UI driver, broad benchmark service, new skill catalogue or model-policy change.
- Changes to the motivating consumer projects, physical devices or private operational requirements.
- Replacing existing acceptance with a smaller test, requiring a single launch for all tests, or mastering an entire interface before delivering its needed scenario.

## Decisions

### 1. Put the trigger in the principles and mechanics in their current owners

Adapt the existing speed/feedback and collaboration paragraphs instead of adding another long checklist. Planning identifies costly unknowns; execution revisits them at an unexpected failure, expensive repetition, coupled correction or growing scope. An internal assessment identifies the next needed fact, the cost of acquiring it and the smaller valid alternative. Only a material change of approach needs a user-facing update.

The critical trigger is a recurring class of difficulty. A newly discovered locator or input failure does not reset the assessment merely because the previous failure was different. One sufficiently informative failure can justify extraction. A long operation with a necessary integration oracle can remain the correct next step.

Extend `project-verification` to distinguish preparation, driver, product and observation failures and select the appropriate feedback boundary. Extend `token-efficient-workflow` only for repeated preparation/session reuse, invalidation and complete cost. Update `docs/agent-delegation.md` with the workstream handoff and runtime ownership rules. Keep failure-preserving reduction in `reproduce-regression` and checkout mechanics in `isolated-worktree`; reference and apply them within their actual scopes. Do not expand the regression skill into a general workflow controller.

Alternative: a new always-loaded optimization skill or orchestrator. Rejected because the gap is the decision to use existing capabilities, and another universal layer would add activation, context and maintenance cost. Externally maintained OpenSpec skills and configuration are outside the change.

### 2. Reduce the problem before optimizing the larger cycle

Use the following choices as judgment aids, not a mandatory state machine:

| Observation | Next useful boundary |
| --- | --- |
| Product action was never reached | Preparation or test-driver operation with an observed postcondition |
| Several unknown interactions share one prepared environment | Bounded investigation covering those interactions and their return paths |
| Corrections affect several coupled concerns | Separate hypotheses with explicit actual dependencies and checks |
| Product fails under a reproducible trigger | Smallest case preserving that trigger and its relevant environment |
| Only a full integration run exposes the remaining question | The required full run, with its original assertions and useful observations |
| A small direct edit already has reliable feedback | Direct completion and applicable checks |

An extracted task may run in the parent or a worker. Its concise assignment contains the result, known inputs, unresolved question, dependencies, resource owner, check and parent consumer. These facts belong in the existing task or brief; no mandatory new file is introduced. New discoveries can change the decomposition without dropping accepted scope.

### 3. Parallelize independent work and serialize shared effects

The parent continues another useful accepted stream when one dependency is under investigation. A separate worker is valuable for independent execution or isolating a noisy research context, provided briefing, verification and integration do not outweigh the benefit. Preserve existing role selection, capability discovery, worker limits and aggregate resource restrictions.

Worktree isolation protects files only. A shared desktop, installed application, service or device has one active owner unless genuinely isolated instances are available. The handoff names the session/input validity conditions and the restoration responsibility. Reassignment uses verified partial work; it does not repeat the worker's investigation merely to reconstruct context.

Alternative: parallel agents for every subtask. Rejected because shared effects and expensive context transfers can increase elapsed time or corrupt the experiment. A single agent can still benefit from decomposition.

### 4. Reuse prepared state with checked boundaries and early integration

For an expensive prepared application, learn only the required route in one valid owned session where feasible. Each reusable action needs a known precondition, observed effect and relevant return path. Preparation stays separate from replay so editing one action does not automatically restart the application. An ended session or relevant build/configuration change invalidates the affected assumptions; a saved identifier is not a live process.

Promote repeatable discoveries into the existing mechanism owner and exercise the actual parent entry point. A report or exploratory script is useful intermediate work; integration and restoration remain explicit unfinished work until performed. Research-only assignments finish when their evidenced answer is consumed by the dependent decision.

Use the existing project memory home for reusable commands, validity conditions and decisions. Keep private data, runtime state and detailed evidence in appropriate local storage. Reuse research and completed checks while their relevant inputs remain valid, including after interruption. Avoid broad hashes, persistent journals or infrastructure before there is a demonstrated consumer.

Alternative: optimize full-run startup first. It remains appropriate when startup itself is the bottleneck and meaningful decomposition is unavailable, but it does not resolve a collection of unexplored interactions.

### 5. Reuse inspected engineering guidance without inferring a speed claim

Research was checked during exploration on 2026-09-12 and remains applicable to this plan:

| Source | Adopted lesson and limit |
| --- | --- |
| [DORA: Working in small batches](https://dora.dev/capabilities/working-in-small-batches/) | Independently verifiable increments shorten feedback; regrouping all work before testing loses that benefit. Team-level findings do not measure this agent policy. |
| [OpenAI: Subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents) | Bounded independent work and concise returned results can isolate context and permit parallelism. Concurrent writes and coordination have costs; preserve this pack's existing model policy. |
| [Microsoft: Invoke Control Pattern](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-implementinginvoke) | Dispatch, selection and completion are distinct, and an element can disappear after invocation. This supports checked state transitions, not an assumption that a particular legacy control supports a standard pattern. |

Reuse of these practices and the installed mechanisms fits the accepted scope. No external package acquisition or new maintained executable is required by this design. If acceptance needs a missing small executable fixture, new maintained code follows the Rust default and the existing test lifecycle; existing transitional runners are not migrated as part of this change.

### 6. Verify behavior with bounded cases and one real consumer

First exercise a small real path: the revised policy identifies a costly prerequisite, an isolated result resolves it, and the actual parent task consumes that result. Expand checks to the rest of the contract after that useful path exists. Use ordinary native sessions and existing probe/capture mechanisms; do not build a general evaluation platform.

The following cases define acceptance. Synthetic cases establish behavioral distinctions, not real-application compatibility. Case A and a real external task establish actual use; they can be the same run when the real task covers the required observations.

| Case | Input and observable acceptance |
| --- | --- |
| A: Successive different failures | A task has costly preparation and several independently discoverable driver failures before its product check. Without a prompt telling it to decompose, the agent identifies the common layer, resolves the needed route in an owned stream, integrates it and completes the unchanged parent check. No repeated full preparation solely to learn each remaining interaction. |
| B: Wrong reduction | A shorter scenario loses the original failure condition. The agent rejects it as proof and retains a meaningful check, then performs required parent acceptance. |
| C: Small direct work | A bounded edit has a cheap relevant check. The agent finishes directly without a worker, new helper or process document. |
| D: Necessary expensive integration | Only the full path observes the unresolved behavior. The agent preserves and runs that check; elapsed time alone does not cause decomposition or weaker assertions. |
| E: Shared resource and recovery | Two streams need one mutable session while independent work is available. One owner controls it, other useful work proceeds within resource limits, and handoff preserves input validity and required restoration. |
| F: Invalidated or resumed state | The session ends or a relevant input changes after an initial result. The agent re-establishes affected conditions, reuses unaffected knowledge and leaves incomplete integration visible. |

For implementation acceptance, use one locally selected real external development task with a current authorized consumer and a genuinely repeated preparation or feedback bottleneck. It need not be a UI task. Freeze its initial inputs and acceptance before the candidate run, and compare against a replayable pre-change workflow or sufficiently complete matched prior observations. Use isolated inputs, configurations and resources; never switch the live control session's global policy to run the baseline. No private consumer name, path, device or data is committed to the pack.

Record total wall time, time to the first useful signal, expensive preparation/full-run counts, independent-stream work, manual interventions, rework and final correctness. Count setup, briefing, waiting, integration, recovery and verification; overlapping work contributes once to wall time. Use available provider usage only with its known limits.

The practical improvement criterion is elimination of avoidable repeated expensive preparation or a shorter useful feedback cycle, with the same accepted result and no unexplained end-to-end slowdown after full costs. Choose a workload where preparation cost exceeds timing noise before comparison. Investigate actual regressions; repeat only for changed inputs, identified noise or unresolved findings. An incomplete baseline or inconclusive comparison stays unfinished, not a fabricated speedup. Long-term reuse is a supported operating behavior; broader long-term savings remain a hypothesis until observed.

Before model-backed acceptance, explicitly enable the existing opt-in probe route under the implementation task's authority. Prompt-loading checks remain model-free where supported. Planning does not execute those probes.

## Risks / Trade-offs

- Additional instructions become overhead -> Consolidate existing paragraphs and keep detailed examples in their owning skill or guide; do not require per-step narration.
- Decomposition delays integration -> Identify the parent consumer when extracting work and make an actual consumed result the first useful increment.
- Warm state conceals a defect -> Validate relevant preconditions and retain required fresh-start and full-path checks; scope reuse to unchanged conditions.
- Worker isolation is only nominal -> Allocate or serialize mutable runtime resources, retain one owner and finish required restoration.
- Behavioral probes pass by following an explicit hint -> Give realistic task inputs and evaluate tool actions/results rather than keyword recitation or a prompt instructing decomposition.
- Benchmark or helper work displaces delivery -> Reuse native execution, cover several criteria in one representative task where practical, and add no evaluation service.
- Other active changes touch the same sources -> Preserve current dirty inputs, make scoped edits, and keep their pending acceptance and task states independent.

## Migration Plan

1. Preserve the relevant pre-change instruction/skill inputs locally and record their identities before implementation; inspect current content so unrelated edits are retained.
2. Update the canonical principles and existing pack-owned skill/guide owners. Record the confirmed decision in the current project decision home during implementation.
3. Verify source/link hygiene and use the supported linked lifecycle to confirm the changed sources are connected. Use a fresh process; do not infer reloading in existing parent or child sessions.
4. Verify initial instruction loading from two external directories, including a repository with its own project instructions. Check relevant skill discovery and preserve project overrides and unrelated tools.
5. Exercise the useful real path and remaining bounded cases, correct findings, and close only tasks supported by actual evidence. Update the existing operation guide with operating conditions and concise evidence limits.
6. Roll back by restoring only this change's source edits from their preserved inputs and starting a fresh session. If link repair was needed, use the existing lifecycle recovery path; do not disconnect unrelated capabilities or stop the active control channel.
