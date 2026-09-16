## Context

See [proposal](proposal.md) for the outcome. The original change was framed
around three-subscription quota failover; the accepted 2026-09-16 reframe
makes asynchronous team-lead development the everyday scenario and moves
feedback-cadence budgets, instruction-refresh succession and quota pacing to
a follow-up change. OpenCodex is retired: Grok runs through the native
`codex --profile xai` provider with the kit-owned compatibility shim, Z.AI
through `codex --profile zai`, and the lead ordinarily through the shared
GPT-6 Astra default.

The verified foundation stays. Installed CLI `0.154.0` passed the owned
two-client/native-child/reconnect contract, the Rust control connection
performs initialization, exact model binding, one tool-using assignment and a
deterministic GPT-refusal-to-successor handoff, and redundant supplied presets
are retired. The
[native contract receipt](../../../docs/rust-native.md#native-task-control-contract)
owns the tested behavior, original failure boundaries and remaining limits;
global activation remains disabled.

Confirmed user decisions (2026-09-16): steering is delivered by the
controller into the executor session and shown in that executor's window, not
by automating keyboard input in TUI windows; the lead merges accepted work
itself; sessions are replaced only at safe boundaries, after an in-flight
tool call completes; executor feedback and lead improvements flow through the
consuming project's board; role selection is kit configuration, not hardcoded
provider responsibilities; the lead role is activated explicitly through a
`team-lead` skill rather than always-on global instructions.

## Goals / Non-Goals

**Goals:** One lead session that specifies, assigns, unblocks, accepts and
merges; executor sessions that implement complete outcomes in isolated
worktrees and visible windows; asynchronous board coordination owned by the
agents themselves; deterministic controller state for dispatch, recovery and
lead succession; validated role configuration; delivery through the global kit
lifecycle with a real consuming project.

**Non-Goals:** Feedback-cadence budgets, account-limit pacing and
token-benefit comparisons; instruction-refresh succession and `codex resume`
contracts; skill and instruction evolution (owned by
`autonomous-skill-evolution`); a new agent framework, hosted scheduler,
recursive worker trees, purchases, provider-dashboard scraping or a
general-purpose orchestration platform.

## Decisions

### 1. Lead judgment with deterministic controller mechanics

The active lead chooses workstreams, assignments and acceptance boundaries. A
small Rust controller owns dispatch records, windows, worktrees, event
correlation and recovery transitions. It does not ask another model to
classify routine events, poll for progress or decide whether a known
unavailable route is available. Session execution stays native Codex through
the verified app-server path; the normal installed launcher connects the
required control path automatically.

### 2. Profiles and role configuration

Kit-owned reusable configuration names the lead profile, the executor
profiles and the maximum concurrent executor count. The existing installation
check validates presence and the positive limit and reports concrete errors
without substitution. Explicit user profile selection retains native
precedence over role configuration. Successor-lead selection also comes from
this configuration; the implemented handoff seed's single hard-coded successor
remains verified evidence, not the contract.

### 3. The `team-lead` skill activates the role

The user-facing entry point is an ordinary Codex session plus the
kit-delivered `team-lead` skill: the user invokes it (or clearly asks for
orchestrated asynchronous development, which the skill description matches)
and then talks to the lead in natural language - status questions, steering
and stop requests. The skill owns the lead workflow instructions: role
configuration discovery, board setup, specification creation, executor briefs
through harness commands, steering, acceptance, merge and explicit stop.
Ordinary sessions carry none of that context, so a small direct task stays
direct. Global instructions add only a pointer to the skill. Harness commands
(`executor spawn`, steering delivery, `task stop`) are the agent interface;
`task stop` and status remain directly available to the user as the emergency
path that works without the lead.

### 4. The board is the agents' protocol

The consuming project's task board (`beads`, the `bd` CLI) carries
asynchronous coordination: stages as epics, specifications as features,
executor feedback as feedback tasks, lead improvements as tasks or OpenSpec
changes. The controller stays board-agnostic - it knows sessions, windows and
worktrees, not board semantics - so the board remains consuming-project state
and no second source of truth appears. The kit delivers board availability and
workflow guidance through its installation lifecycle and verifies them from a
fresh external session. Public kit sources use synthetic examples only. The
`beads` dependency requires the standard proportional assessment before
execution (identity, license, maintenance, install effects, vulnerabilities).

### 5. Executors are isolated and visible

Every executor runs `codex --profile <id>` in its own visible terminal window
and its own Git worktree created from the task's base revision. The
controller owns the worktree lifecycle: create, map to the assignment,
preserve through interruption, retire after the lead merges or explicitly
discards. Executors never write to the shared checkout. Views are established
before the first model request and show assignment, role, effective
profile/model/effort and live activity; reuse the owned native-console
approach and view-loss behavior from the verified contract. A switchable list
or hidden session does not qualify.

### 6. Steering through the session channel

Lead steering is delivered by the controller into the executor session and
appears in that executor's window (option "b" from the exploration). Keyboard
automation of TUI windows and hidden background sessions are rejected:
fragile, unauditable, and invisible to the user. Waiting and routine event
handling make no model calls; no status polling. Executor escalations travel
as board feedback tasks with bounded context, keeping the live channel for
steering and the board for durable asynchronous records.

### 7. The lead accepts and merges

The lead reviews completed assignments against requirements and applicable
checks, returns concrete defects to the original executor, and merges accepted
branches itself. Worktrees are retired after merge or explicit discard;
rejected work keeps its partial result until resolved. Acceptance and merge
are recorded in task state and reflected on the board, without claiming
completion for integrated work whose checks have not passed.

### 8. Persist the minimum state needed to continue safely

Use existing host-private harness storage and serialization conventions. A
task record keeps stable identity, workspace and worktree mapping, accepted
objective/constraints/authorization, decisions, lead binding, assignments,
resource owners, attempt identities, result/check references and next action.
Raw model history, credentials and private consumer evidence stay outside
shared source. Persist before dispatch and ownership transitions; reconcile
session status, process liveness and worktrees after restart. A late result
is correlated to its attempt and retained, not blindly applied.

### 9. Availability and succession at safe boundaries

| Observation | Controller action |
| --- | --- |
| Healthy worker takes time | Preserve owner; wait on events without model polling. |
| Confirmed quota exhaustion | Exclude affected route for a bounded window, preserve work, reassign within configured profiles. |
| Temporary throttling | Respect usable retry guidance without assuming weekly exhaustion. |
| Auth or exact-model rejection | Retain original cause, exclude affected route until conditions change; no credential changes. |
| Transport failure | Reconcile in-flight effects and use bounded runtime recovery; do not mark provider quotas exhausted. |
| Empty/intermediate completion | Inspect visible work and result delivery; continue or reassign with a concrete diagnosis. |

Lead succession replaces the failed lead with one configured capable
successor at a safe boundary - never mid-tool-call - preserving decisions,
assignments and acceptance. The configured preferred lead returns at a safe
decision boundary after verified recovery. Instruction-refresh succession via
`codex resume` (including deterministic session selection and verification
that a resumed process reloads current instructions and skills) is owned by
the follow-up change.

### 10. Reuse current requirements and research

The archived decomposition requirements now live in the main
`agent-delegation` capability; this change modifies them in place and does
not reopen their resource-ownership, validity or restoration scenarios. The
following current sources support the selected mechanisms; installed help and
isolated behavior determine actual compatibility:

- [Codex subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents): native agents and scoped context remain the execution basis; no second agent framework.
- [Codex app-server](https://learn.chatgpt.com/docs/app-server): native session/turn control and local transport; experimental status requires version checks and contract tests.
- [Beads](https://github.com/gastownhall/beads): agent-first graph tracker with a non-interactive CLI and git-friendly storage; suitability on Windows and lifecycle integration require the task-owned assessment before execution.

The implemented transport uses pinned `tungstenite` 0.30.0 with only its
handshake feature. Its
[upstream manifest](https://github.com/snapview/tungstenite-rs/blob/v0.30.0/Cargo.toml)
identifies MIT/Apache-2.0 licensing and Rust 1.85 compatibility. Reuse avoids
implementing WebSocket framing or introducing a separate async runtime, TLS
stack or platform-specific WinHTTP adapter for this local endpoint. The
lockfile records the full dependency set. Inspection on 2026-09-12 covered
all 18 added packages, their upstream/license metadata, and the two build
scripts; an OSV batch lookup returned no listed vulnerabilities for those
versions. The earlier
[handshake denial-of-service advisory](https://rustsec.org/advisories/RUSTSEC-2023-0065.html)
is fixed in this version. Private receipts retain the query and metadata;
this is scoped dependency evidence, not a security guarantee. Any further
dependency, including `beads`, still requires the same proportional
assessment before execution.

## Risks / Trade-offs

- External `beads` dependency -> proportional assessment, supported-version pinning, lifecycle check, and explicit board-unavailable behavior instead of silent degradation.
- Experimental native control contract -> bind acceptance to the tested CLI/schema and exact behavior; unsupported installations keep their previous usable path with an explicit limitation.
- Worktree sprawl and stale branches -> controller-owned lifecycle, retirement after merge or discard, preservation on interruption, no shared-checkout writes.
- Two coordination planes (board and live channel) -> the board is the durable asynchronous record, the channel is live steering; neither duplicates the other's state and the controller parses neither board content nor transcripts.
- Parallel executors share account windows -> concurrency comes from validated configuration; budget measurement is deliberately deferred to the follow-up change, so this change makes no savings claims.
- Publication boundary -> synthetic consuming-project examples only; private paths, identities and evidence stay in host-private storage.

## Migration Plan

1. Add validated role configuration and board availability in owned isolated state; assess the `beads` dependency before execution.
2. Prove integrated executor dispatch: own window, own worktree, configured profile, live steering through the verified control path.
3. Add lead acceptance/merge, durable task records, cause-aware recovery and configured lead succession.
4. Connect controller, views, configuration and board guidance through native install/update/check/recover/disconnect in isolated installations.
5. Validate a real external consuming development task and update owning guides and decision records; only then claim completion.
6. Rollback removes dispatch authority and owned links without erasing private checkpoints, worktrees or unrelated runtimes; explicitly stopped tasks stay stopped.

## Open Questions

- Exact non-interactive `bd` command contract on Windows (owned by the board
  integration task).
- Preferred-lead return boundaries when several executors are healthy.
- The follow-up change owns: deterministic `codex resume` session selection
  and instruction-reload regression checks, telemetry ingestion from provider
  dashboards versus native limit reads, feedback cadence and token-benefit
  acceptance.
