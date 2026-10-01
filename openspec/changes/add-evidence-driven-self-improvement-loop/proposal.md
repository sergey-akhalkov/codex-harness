## Why

The harness can account for attempts and retain feedback, but it does not yet turn observed losses into a continuous sequence of independently evaluated improvements. Real improvement work can provide useful comparison tasks: evaluate candidate A by implementing B with and without A, retain B, then evaluate B on C instead of maintaining a separate synthetic coding benchmark.

Improvement also includes subtraction: accumulated features, instructions and machinery can consume context, operations and maintenance without a corresponding benefit. Occam's razor favors the simplest sufficient solution with supported usefulness; neither low usage nor fewer files alone proves that a capability should disappear.

## What Changes

- Deliver a `self-improvement-loop` skill and resumable Rust controller for an explicitly started, continuously operating A -> B -> C loop. The change includes installed operation outside this checkout, not only repository scaffolding.
- Admit hypotheses from attributable execution evidence, reproducible defects and verified tool constraints. Require a causal explanation, predicted benefit, counterexample and prior-result search; do not generate changes solely to keep the loop occupied.
- Consider retaining, simplifying, consolidating, loading on demand, disabling or removing existing capability alongside additions. Review attributable overhead in skills, code/features, instructions, documentation, tools/dependencies, configuration, checks, orchestration and retained outputs without creating a separate recurring audit system.
- Require explicit, informed user approval before implementing a proposed removal of code, features or skills, including experimental removals and disabling that withdraws capability. Present exact scope, evidence, expected benefit, lost scenarios, alternatives and recovery first; a favorable experiment or general permission to run the loop is not removal authority.
- Use Beads `task` items labeled `hypothesis` as the sole durable owner of hypothesis identity, queue, lifecycle and decisions. Reuse existing feedback/benefit bookkeeping and local evidence storage rather than introduce a parallel journal.
- Require a linked OpenSpec change with proposal, requirements, design, implementation tasks and experiment acceptance before implementing **every** hypothesis, including instruction, skill, MCP, plugin and configuration changes.
- Preserve paired comparison of frozen baseline and candidate runtimes on the same frozen real task with fresh executors, independent acceptance and complete accounting. Proxies or different-task comparisons do not replace this method. Retain useful target implementations without confusing their correctness with evidence of their own benefit.
- Separate observed end-to-end cost from work metrics adjusted automatically for evidenced external infrastructure waits, with replayable attribution and reconciled totals. Control initial cache/warm-up state, execution order and relevant load; retain network failures, downstream waiting effects and model variation. Distinguish measurement uncertainty from run-to-run variability, bind conclusions to supported bounds and declared statistical evidence, and verify the adjustment with independent positive and negative controls. Preserve actual effects on build demand, caching, polling, recovery and model strategy.
- Keep each candidate on its own branch and owned Git worktree, reusing an existing eligible worktree when safe. Select prepared baseline/candidate runtimes without reverting source or rebuilding unchanged artifacts; merge candidate code into the accepted mainline only after supported benefit and integration checks.
- Qualify the user-supplied local model through repeated agent/tool execution before comparisons. The selected policy checks required solution equality and server/client parameters available through the API, retaining unavailable weight hashes and hardware details as limits. Never silently change that policy, substitute another model or change billing route. Exact deployment inputs stay private.
- Record `adopt`, `reject` or `inconclusive`, preserve prior evidence and permit reconsideration only on a recorded new basis. Confirm promising effects on retained real tasks when the declared scope requires it.
- Support stop/resume, interruption recovery, bounded resource use, visible model conversations and evidence-backed experimental-baseline advancement. Keep live installation publication within separately established scope and lifecycle authority.

## Capabilities

### New Capabilities

- `self-improvement-loop`: Evidence-driven hypothesis selection including simplification, informed removal approval, mandatory per-hypothesis planning, sequential real-task evaluation, continuous execution, recovery and global skill/CLI delivery.

### Modified Capabilities

- `orchestration-feedback-loop`: Beads ownership of hypothesis cards, planning prerequisites, nonblocking evaluation relationships and durable, evidence-linked benefit and removal-authorization decisions.
- `harness-outcome-evaluation`: Frozen task/runtime comparisons, exact candidate lineage, local-runner repeatability qualification, simplification benefit/retained-behavior checks and accounting for rolling real-work experiments.

## Impact

Extend the existing Rust CLI, `harness-core` outcome and board modules, native attempt runner and their tests. Reuse the shared rollout reader, independent acceptance, process observation, visible dispatch, skill packaging and installation lifecycle. Add the owned skill and update the relevant operating guides during implementation. No new tracker, external benchmark service or first-party scripting runtime is required. This planning change does not run models, create operational board tasks, alter installed settings or implement the loop.
