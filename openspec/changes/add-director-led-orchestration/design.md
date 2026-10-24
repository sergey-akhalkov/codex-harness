# Design

## Context

See [proposal.md](proposal.md) for motivation and agreed scope. The final agreement requires a prepared OpenSpec change for every delegated Lead; the Director remains free to work directly or choose direct Executors. Leads and Executors use `ds`, while the root remains ordinary Codex. The user selected explicit restoration after CLI closure and direct-child defaults of two Leads and four Executors per Director, with six Executors per Lead.

Inspected implementation foundations:

- [executor_mcp.rs](../../../crates/codex-harness/src/executor_mcp.rs) publishes a shared tool list, derives callers from injected native metadata, refuses descendant dispatch by a recorded Executor, and retains run/lease receipts. Its current role model distinguishes only Lead and Executor.
- [executor_control.rs](../../../crates/codex-harness/src/executor_control.rs) starts native threads on the existing app-server with profile-derived configuration overrides. [executor_cli.rs](../../../crates/codex-harness/src/executor_cli.rs) prepares one repository's pooled checkout and structured assignment. Executor dispatch already sets `agents.enabled=false`.
- [task_worktree.rs](../../../crates/harness-core/src/task_worktree.rs) owns repository identity, pool leases and preservation; [task_orchestrate.rs](../../../crates/harness-core/src/task_orchestrate.rs) owns existing integration/recovery operations. Reuse those owners instead of introducing another workspace manager.
- [orchestration.toml](../../../global/orchestration.toml) currently routes the root through `default`, Executors through `ds`, and explicitly ships executor and pool limits of six. The existing specification's four-slot heading is older than that configuration; this change supplies coherent new defaults and retains the four-slot scenario as an explicit local override case.
- Existing tests cover MCP authority and closed schemas in [executor_mcp.rs](../../../crates/codex-harness/tests/executor_mcp.rs), structured inputs in [executor_assignment.rs](../../../crates/codex-harness/tests/executor_assignment.rs), and pool behavior in [worktree_pool.rs](../../../crates/codex-harness/tests/worktree_pool.rs). They do not establish the proposed three-level behavior.

The installed CLI inspected during exploration reported version 0.164.0. Official [MCP configuration documentation](https://learn.chatgpt.com/docs/extend/mcp?surface=cli) documents tool allowlists for plugin servers, and [app-server documentation](https://learn.chatgpt.com/docs/app-server) documents native thread lifecycle. Those contracts and source inspection do not prove that distinct role tool surfaces survive all start/resume paths in one app-server; the first implementation check exercises that exact boundary.

## Goals / Non-Goals

**Goals:** Implement the behavior in [director-orchestration](specs/director-orchestration/spec.md) by extending existing native transport, configuration, board and worktree owners. Keep specification identity, runtime identity and Git lineage explicit and separate. Preserve the current direct-Executor path as a useful supported route.

**Non-Goals:** See the proposal. This design adds no general DAG scheduler, mirrored board database, background daemon, recursive delegation framework or per-role MCP implementation. Runtime role enforcement is an orchestration boundary, not an OS sandbox against an unrestricted local process.

## Decisions

### 1. Three runtime roles, two configured model routes

The runtime recognizes Director, delegated Lead and Executor independently of profile names. The root receives Director authority from its verified launcher/native registration. Only Director dispatch can create a Lead; only Director or Lead dispatch can create an Executor. A profile named `ds` does not imply any authority.

| Role | Shipped route | Creates | Owns acceptance |
| --- | --- | --- | --- |
| Director | ordinary Codex, existing root settings | Leads or direct Executors | Assigned specs and combined outcome |
| Lead | `ds` | Executors | Its specification and Executor outputs |
| Executor | `ds` | Nothing | Reports evidence to its parent |

Keep `lead_profile` and `successor_lead_profile` as compatibility names for root routing. Add `delegated_lead_profile`; do not reinterpret an existing root profile as a child profile or make a child inherit root succession authority. The new setting ships as `ds`, as do existing Executor profiles. Preserve the actual profile's model, effort, provider, authentication and feature settings; retain DeepSeek's existing Code Mode compatibility setting. No silent route or billing fallback is introduced.

This is smaller and safer than switching the shared `ds` configuration between roles or creating separate model profiles merely to grant different tools.

### 2. Lead admission consumes a prepared specification

Lead dispatch takes a named OpenSpec change, its resolved planning home and committed artifact inputs, code repositories/bases, a beads issue reference, and concise outcome/acceptance context. The planning home can differ from a code repository; its selected source and revision remain explicit. Follow the selected change's existing schema, including conditional design, rather than modifying OpenSpec workflows or requiring invented paperwork.

Mechanical validation checks the reference, applicable artifact completeness, declared paths and exact committed inputs before allocation or model effects. The Director owns semantic readiness: real requirements, boundaries and testable success conditions, with no unresolved consequential choices hidden in a title. Passing `openspec status` alone cannot establish this readiness. Lead briefs reference the existing artifacts and preserve applicable constraints instead of copying the Director's conversation.

The Lead implements its assigned change and can refine execution details and task decomposition within that scope. Changed requirements return to the Director and use an explicit revised committed input. A direct Executor retains the existing bounded assignment contract and does not need an artificial change solely for dispatch.

This preserves Director freedom while rejecting the earlier, superseded option of assigning a Lead a specification-free block.

### 3. One plugin, session-specific tools and server authority

Extend [.agents/plugins/executors](../../../.agents/plugins/executors/.codex-plugin/plugin.json). Add a Director-only Lead-dispatch operation. Reuse existing Executor dispatch and native control mechanics; expose an unambiguous parent-message/question operation for Leads, which both supervise children and report upward. Preserve existing Executor operations and result fields as compatible entry points.

At native thread creation and restoration, apply the role's effective plugin tool allowlist and role instructions to that thread. Do not rewrite global MCP configuration or the shared `ds` profile. Leads and Executors retain `agents.enabled=false`: a Lead's sole spawning path is its permitted Executor MCP operation. Executors see parent communication and own-run observation, with no child management operations. Directors see both dispatch levels and their tree controls.

Every call still verifies injected caller identity against the recorded role, root, parent, generation and leases. Filtering is not authorization. Reused slot paths, copied run IDs, cwd and selected model cannot establish authority. Directors can observe/control their tree; ordinary steering should follow its responsible Lead to avoid contradictory instructions, without a technical rule forcing every Director task through a Lead. Other roots and sibling teams cannot acquire authority by supplying identifiers.

First verify actual session/provider tool catalogues, successful Lead-to-Executor dispatch and refusal of direct forbidden calls on the installed native runtime. If its per-thread contract is insufficient, report the exact gap and preserve the candidate; do not quietly substitute another runtime, model or prompt-only restriction.

### 4. Extend existing durable run records

The existing private runtime owner records a run's role, root, immediate parent, current native thread/generation, exact route, board/change references, selected repository/workspace bindings and correlated pending operations. Board references are pointers, not another copy of task state. Spec references identify frozen inputs, not another requirements store. Native history remains the conversation owner.

Use stable dispatch request identities scoped to the verified parent. Validate intent first, reserve parent/shared capacity, persist preparation, prepare the required workspaces, establish the native thread and role surface, then submit the turn. Persist enough identity before model effects to reconcile a lost response. Equal retries return the original operation; conflicting reuse fails. A crash after possible native acceptance preserves reservations and work until reconciliation establishes what happened.

Use the existing staged records and locks with versioned role metadata, not a new database or daemon. Old root-Lead-to-Executor records map to Director-to-Executor without promoting a worker. Reject unsupported or uncertain record versions before effects and retain their recovery evidence. The logical coordinator can be shared by existing MCP processes through these records; it need not be a separate service.

### 5. Parent limits compose with existing shared limits

The intended shipped values are:

```toml
lead_profile = "default"
successor_lead_profile = "default"
delegated_lead_profile = "ds"
executor_profiles = ["ds"]

director_max_leads = 2
director_max_executors = 4
lead_max_executors = 6

max_concurrent_executors = 16
worktree_pool_size = 18
```

The first three limits apply to immediate children of each parent. Grandchildren do not consume the Director's four direct-Executor positions. The existing executor limit remains an installation-wide envelope across parents and repositories. Leads count against their Director's Lead limit and their worktree leases, not the Executor envelope. Existing heavy-command resource admission remains independent.

Admission includes startup reservations, active runs and runs awaiting replies. Reconcile stopped or terminal effects before releasing concurrency; completed but unaccepted results still hold their worktrees. Acquire shared and parent reservations atomically, and acquire multi-repository pool locks in stable repository order. The full default tree is `2 * 6 + 4 = 16` Executors plus two Leads, hence eighteen workspaces in a single repository. Pool positions remain lazy and reusable.

Explicit lower local envelopes or pool capacities are valid operating choices. Check/status reports the reachable limits and current bottleneck; dispatch names the binding constraint. Lowering limits never kills healthy runs. New limits do not authorize tasks, and capacity cannot justify creating unnecessary work. Larger role limits can require deliberate adjustment of the separately configured shared envelope and pools.

### 6. A Lead workspace is a repository set

Use an explicit mapping from repository identity to input commit, integration target and managed worktree. Each selected repository has one Lead integration worktree; an Executor receives the worktrees needed for its owned slice. The primary checkout supplies the native cwd; additional declared repositories remain explicit allowed workspace inputs, not guessed siblings. Validate the native permission configuration and code-tool navigation for the entire declared set before dependent implementation.

Worktrees are neighboring slots of their repositories' common pools, not physically nested directories. A child is derived from a committed parent revision, not from the parent's uncommitted filesystem. Resolve that revision automatically from the declared assignment inputs and report the actual base. Legacy direct-Executor default-base behavior remains supported; hierarchy dispatch supplies the required explicit base rather than silently changing that legacy default.

Multi-repository preparation either establishes all required bindings before starting the model or reports the exact partial preparation and safely releases only unused reservations created by that attempt. Never reset pre-existing work to simulate atomic preparation. Tool connections with mutable project selection must be session-scoped or explicitly addressed, so one Lead cannot retarget another Lead's Serena/project context.

### 6.1 Hierarchy labels are stable pool positions

The shared pool has eighteen lazy positions. Their hierarchy labels are fixed: positions 1-4 are `de1..de4` (Director Executors), 5-6 are `l1..l2` (Leads), 7-12 are `l1e1..l1e6`, and 13-18 are `l2e1..l2e6`. The same label is used for the corresponding worktree in every repository selected by a multi-repository assignment. The label describes the pool position and hierarchy owner; it is not authority.

Existing `{repo}-wtN` worktrees and records predate the labels. Keep them readable and do not move a held or active worktree. A free or released legacy position may be migrated to its hierarchy label only after its owner and partial work are proved safe. Compatibility CLI and ordinary task allocations may continue to use legacy labels until their positions are migrated, so the pool must recognize both names for a position without treating one as a foreign worktree.

### 7. Beads and OpenSpec retain their own responsibilities

Use a selected existing board for a linked initiative: an epic for its outcome, a feature per assigned change and tasks for Executor work. Reuse existing suitable cards. Separate initiatives can retain different boards; the Director aggregates observations without a mirrored tracker. A multi-repository spec keeps one coordinating board with explicit code-repository references.

Bind board location and issue identity into each assignment independently of cwd. Use run-specific assignment/review ownership rather than a globally ambiguous `assignee=lead`. Agents use the board CLI for durable assignments, decisions and feedback; the Rust runtime stores references and does not parse or duplicate the board. Existing `lead_review` can mean review by the recorded immediate parent; a new status for every hierarchy level is unnecessary.

The Director coordinates dependencies between specifications. Leads coordinate the tasks within their own specification. Dependencies are board relationships and agent decisions, not inferred from parent-child membership. Shared service and installation ownership follows existing serialization rules.

### 8. Integrate and accept at the right boundary

Executors return actual changed paths, revisions, checks, limitations and evidence to their immediate parent. A Lead integrates and checks its specification in its owned workspace set. The Director consumes that result, integrates with other accepted specifications and tests the combined user path. Return bounded corrections to the owner rather than duplicating its investigation.

Multi-repository evidence records a revision vector and integration progress for each repository. Git cannot provide atomic cross-repository merge; a partial result remains explicit and recoverable. No automatic remote publication or deployment is implied. Closing an Executor task does not close its specification. A Lead result remains pending Director acceptance, and acceptance does not silently release a worktree needed by unfinished descendants or integration.

### 9. Explicit restoration and addressed subtree control

Maintain one native CLI surface and its app-server. Titles and status identify role and parent; the native view may be a flat list if those relationships remain inspectable. Wait operates across a parent's direct children of either role; the Director can inspect descendant status without acknowledging a result for its Lead. Retain existing quiet waits, request correlation and acknowledgement semantics.

Closing the CLI stops new dependent dispatch and preserves hierarchy state for explicit resumption. Do not add unattended continuation. On ordinary close, interrupt owned work through existing containment. After abrupt failure, reconcile processes, pending native operations, partial files and interrupted checks before any new launch. Reopening provides inspection; explicit resume verifies bindings and resumes the appropriate existing threads or performs supported fresh recovery from visible evidence.

A Lead stop atomically closes admission for its subtree, reconciles starts in progress, and interrupts the Lead and its descendants without waiting for a model to cooperate. A direct Executor stop remains local. Report partial interruption and surviving commands separately from observation failure; never claim rollback. Do not release a parent workspace while unfinished children depend on it. Resume a stopped branch only by explicit instruction, with the same route and preserved assignment scope.

## Risks / Trade-offs

- Role filtering may differ across native start/resume and provider routes -> verify real catalogues and forbidden calls before building the wider hierarchy; keep capability failure explicit.
- `ds` Lead coordination may require correction on difficult specs -> keep outcome-level acceptance with the Director, return concrete corrections and escalate capability issues without silently changing models.
- More sessions can increase total cost even with the same model -> keep delegation discretionary and distinguish orchestration capability from measured efficiency; no token or quota savings are promised.
- Same-repository Leads can produce behaviorally incompatible changes -> declare dependencies/ownership and test the combined revision set, including a counterexample where both local checks pass but integration fails.
- Fan-out can exhaust pools or shared resources -> enforce both parent limits and shared envelopes atomically; preserve smaller local settings and expose the bottleneck.
- Shared profile or MCP project state can leak across sessions -> use per-thread overlays and explicit workspace bindings, with simultaneous-session acceptance.
- A crash can occur between native acceptance and receipt observation -> persist preparation identity and reconcile retries, retaining uncertain effects rather than reallocating blindly.
- Beads updates and Git integration are not one transaction -> record actual parent acceptance and applied revisions in their existing owners and reconcile partial outcomes before closure.

## Migration Plan

1. Establish the three-role native vertical path through owned fixtures, then a bounded actual ds Lead/Executor interaction. This is the first usable result, not final delivery.
2. Extend configuration and run-record readers before enabling new dispatch. Materialize missing hierarchy fields through the owned lifecycle. Fresh installs use the coherent defaults above; recognize prior kit-owned shipped defaults when updating them, preserve deliberate local overrides and state their effective constraints. Never shrink or reset existing pools as a side effect.
3. Add multi-repository workspace sets, full hierarchy admission, communication and recovery through their current Rust owners. Preserve legacy direct Executor calls and existing records; validate changed authority on every continuation.
4. Reconcile owned role files, `team-lead`, `board-workflow`, generated briefs, help and operating documentation. Do not edit externally maintained OpenSpec skills, schemas or templates. Keep one authoritative home for each contract.
5. Run focused native tests, then the existing isolated installation/update checks and a fresh installed external consumer acceptance. Use deterministic fixtures for full 2/4/6 races and failure cases; use real configured sessions and a locally supplied real cross-repository task to establish actual consumption. Keep private task identities and raw evidence outside the kit.
6. Publish through the existing supported lifecycle only after checks pass. If unrelated consumers block activation, preserve the verified candidate and report the dependency. Rollback restores the previous supported installation without deleting new run records or workspaces; an older runtime must refuse unknown hierarchy state, and restoration uses a version that understands it.

The implementation tasks retain the complete outcome. Passing the first native slice or planning validation does not complete hierarchy delivery.
