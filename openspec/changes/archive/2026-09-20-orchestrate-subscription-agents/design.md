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
[native contract receipt](../../../../docs/rust-native.md#native-task-control-contract)
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
small Rust controller owns dispatch records, windows, native worktree mapping and retirement, event
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
`beads` assessment and pin are recorded in
[project decisions](../../../../docs/project-decisions.md#orchestration-board).
Verified Windows v1.3.0 non-interactive contract: `bd init --skip-agents
--non-interactive --quiet` (no `AGENTS.md`); stages as `bd create -t epic
--json`; specifications as `bd create -t feature --parent <epic> --json`;
executor feedback as `bd create -t task --labels feedback --parent <feature>
--json` (`-t feedback` is rejected); status/report as `bd status --json`,
`bd list --label feedback --json`, `bd epic status --json`, and `bd close
--reason --json`. After init, `bd -C <project>` works; it cannot be used for
`init`. There is no `bd report` command (`status`/`stats` is the overview).

### 5. Executors are isolated and visible

Every executor runs `codex --profile <id>` in its own visible terminal window
and a Codex-managed Git worktree created from the task's base revision.
This is not an ordinary Git worktree from `isolated-worktree-workflow`:
executor isolation uses the CLI-managed pool, while intra-session independent
edits keep using ordinary Git worktrees. Installed CLI 0.154.0 remains the
inspected allocation owner: experimental feature `worktrees`, flag
`--worktree` / TUI `/worktree`, detached HEAD from the source commit,
checkout under the native pool (`$CODEX_HOME/worktrees` or the configured
Desktop root), thread bound before the first turn. CLI allocations are not
auto-cleaned, do not copy uncommitted or ignored files, and reject ephemeral
sessions, `--ignore-user-config`, code review, and `exec resume --worktree`.
Interactive `--worktree` also rejects `--remote`, so the verified harness
control view must not pass `--worktree`; the controller allocates or reuses
the native checkout, records the mapping (owning thread including 0.155.0
title/archived/unavailable status, root, cwd, source, head), then attaches
the `--remote` TUI with that cwd. Resume does not pass `--worktree` again.
Spawn_agent helpers stay ephemeral in the parent executor checkout and must
be visible; they do not get a second `--worktree`. Add harness mapping only
for those native gaps. Do not invent a second worktree tree. Unsupported or
disabled `worktrees` is an explicit limitation, never a shared-checkout write
and never a silent fallback to an ordinary Git worktree.

CLI 0.155.0 adds owner details in the managed-worktree browser and confirmed
deletion of a clean managed worktree in the current repository. Native delete
defaults to Cancel, preserves thread history, and refuses the current
checkout (including path aliases) plus any tree with local, untracked or
ignored changes. After merge or explicit discard, retire through that native
delete when those preconditions hold. When they do not - typical once an
executor has ignored build outputs - preserve the checkout and report the
limit; do not force-remove, and do not treat agents-overview hide, archive
or task deletion as worktree retirement. Desktop
[worktrees](https://learn.chatgpt.com/docs/environments/git-worktrees) remain
a distinct product: `.worktreeinclude` and auto-delete still do not apply to
CLI allocations. The lead still merges accepted work; because the checkout
is detached, merge creates a branch or cherry-picks rather than checking the
same branch out in two trees. Views are established before the first model
request and show assignment, role, effective profile/model/effort and live
activity; reuse the owned native-console approach and view-loss behavior
from the verified contract. A switchable list or hidden session does not
qualify.

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
branches itself. Worktrees are retired after merge or explicit discard using
native confirmed deletion when the 0.155.0 clean-managed preconditions hold,
otherwise preserved with an explicit limitation; rejected work keeps its
partial result until resolved. Acceptance and merge are recorded in task
state and reflected on the board, without claiming completion for integrated
work whose checks have not passed.

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
- Installed Codex CLI managed worktrees: `codex --worktree`, `codex features` (`worktrees` experimental, default off), TUI `/worktree`. Inspected 0.154.0 help and rust-v0.154.0 sources (#42652, #43069, #43286) for allocation; rust-v0.155.0 (#43942, #44424, #44433) for owner details and confirmed deletion of clean managed worktrees. `--worktree` with `--remote` is rejected (`startup_orchestration.rs`). `codex exec --worktree` rejects `--ephemeral`, `--ignore-user-config`, review and `exec resume`. CLI `WorktreeSettings::for_cli` shares the Desktop pool and disables automatic cleanup. Native 0.155.0 delete is not auto-cleanup: it requires a clean managed worktree of the current repository and refuses ignored files. Desktop [worktrees](https://learn.chatgpt.com/docs/environments/git-worktrees) remain a distinct product: `.worktreeinclude` and auto-delete do not apply to CLI allocations. Ordinary Git worktrees stay owned by `isolated-worktree-workflow`.
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
- Worktree sprawl -> native CLI allocations are not auto-cleaned. 0.155.0 confirmed deletion only covers strictly clean managed trees, so ignored executor artifacts typically block it; the controller records native owner identity, retires through that delete when eligible, otherwise preserves the checkout, and does not create a second harness tree, force-remove dirty trees, or check the same branch out twice.
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

- Windows allocation of a managed worktree in an owned isolated `CODEX_HOME`
  (task 4.1), including bind-before-first-turn, 0.155.0 owner identity, and
  leftover checkout cleanup when native confirmed deletion does not apply.
- Preferred-lead return boundaries when several executors are healthy.
- The follow-up change owns: deterministic `codex resume` session selection
  and instruction-reload regression checks, telemetry ingestion from provider
  dashboards versus native limit reads, feedback cadence and token-benefit
  acceptance.
