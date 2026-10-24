# Agent selection and visible conversations

Selection has two paths; never mix them. Ordinary in-session subagents take
explicit `model` and supported `reasoning_effort` parameters per assignment;
GPT-6 Astra at `xhigh` leads, and DeepSeek V4.1-Flash at `max` executes
delegated work; xAI and Z.AI models are not used. Kit executor dispatch takes no
per-assignment model, provider or effort: it runs exactly the configured
executor profiles, and the profile's configured provider, model and reasoning
effort are that assignment's complete explicit selection. The kit configures
one executor, `ds`, binding DeepSeek V4.1-Flash (`deepseek-flash`) through
provider `deepseek` at effort `max`; no other model, provider or billing route
is a fallback. One executor profile is normal full capacity, not a delegation
limit. Choose from task
complexity, risk and the full cost of the accepted result; a separate TOML
file per effort or activity is unnecessary. Supported efforts differ by
model; verify the effective binding instead of assuming every level exists on
every route.

The DeepSeek profile must set `[features] code_mode = false`: its
[Responses API](https://api-docs.deepseek.com/guides/responses_api/) accepts
`apply_patch` as a custom tool but rejects Code Mode's `exec`. Ordinary command
and function tools remain available. Keep this compatibility setting in the
local `ds.config.toml` profile; the shared default can keep Code Mode enabled.

The launcher also routes DeepSeek requests through the kit-owned loopback
Responses shim. DeepSeek rejects an input when Codex inserts a developer, user
or reasoning item between a tool call and its output; the shim moves the
matching outputs ahead of those inserted items before forwarding to
`https://api.deepseek.com`. The route is selected from `--profile ds` and from
the `-c model_provider="deepseek"`/`-c model="deepseek-flash"` overrides used by
managed app-server sessions. Restart a saved thread after installation so its
next app-server uses the corrected route.

The removed Astra names migrate to these direct arguments. This table preserves
their former meaning; it does not prescribe an effort for every new assignment.

| Former purpose | Retired name | Model argument | Effort argument |
| --- | --- | --- | --- |
| Backup middle for a hard slice | `middle_backup` | `gpt-6-astra` | `high` |
| Main session; a separate hard slice when needed | `senior` | `gpt-6-astra` | `xhigh` |
| Rare consultation on a hard intellectual blocker | `principal` | `gpt-6-astra` | `max` |

For example, use an ordinary agent with `model="gpt-6-astra"` and
`reasoning_effort="high"` instead of `agent_type="middle_backup"`. Those
OpenCodex role files are retired. The former `grok_reviewer` remains retired.

Executor threads are native threads on the lead's app-server, and the intended
inspection surface is Codex's native `/agents` view with a distinct title, its
conversation, model/effort and state; the lead keeps one CLI view and does not
open per-executor terminal tabs, panes, consoles or a file relay. Visual
`/agents` presentation was confirmed for the installed two-executor proof, and
`executor_status`/`executor_wait` remain the bounded machine-readable state and
result paths when that view is unavailable. Include model-backed helpers, make
leadership changes explicit, and inspect a run's conversation in the same
native view when available. `/agents` navigation is observation only: opening,
focusing or closing its view does not start, complete, stop or resume a run.

Do not resize, move or arrange desktop windows - including the lead's own
terminal - so sessions fit the screen. Do not capture Codex or sibling agent
windows with Nuphus screenshots, and do not poll window pixels for status.
Identify non-Codex windows with list, title, bounds and state; keep screenshots
for genuine visual questions about owned UI. If a required native view is
unavailable, report that capability gap; `executor_status` and `executor_wait`
remain the bounded state and result path.

## First use: Director to Lead to Executor

Use this sequence for the first prepared hierarchy after installation or update:

1. Start a fresh Codex CLI session so the installed executors plugin, role
   guidance and `/agents` view are loaded. A running session does not reload
   them. The Director verifies that both `lead_dispatch` and
   `executor_dispatch` are visible. A missing tool or view is a capability
   failure with an installation/restoration action, not a reason to substitute
   another runtime, model or tracker.
2. Commit the prepared OpenSpec change in the selected repository and record
   its existing board task. The Director calls `lead_dispatch` with the change,
   exact base, applicable committed inputs, declared repositories, output,
   acceptance, consumer and board reference. A title, board card or dirty
   worktree is not a prepared change.
3. Before relying on the run, verify the dispatch receipt, `executor_status` and
   `/agents` agree on the run and role; profile `ds` with provider `deepseek`,
   model `deepseek-flash` and effort `max`; immediate parent, root and native
   thread; allocated checkout and slot; and exact committed base and input
   revision. Keep credentials and raw runtime identifiers in private state,
   not public prose.
4. The delegated Lead has the executor dispatch/control tools and
   `executor_ask_lead`, but no `lead_dispatch`. It calls `executor_dispatch`
   for each owned slice from an exact committed input revision, then reviews the
   returned diff and checks and accepts or returns it. An Executor has
   `executor_status` and parent communication through
   `executor_ask_lead`/`executor_message`, but cannot dispatch descendants.
5. When the Lead reports the combined result, the Director reviews the
   integrated revisions and check evidence before accepting and reporting
   upward. Result delivery, review, acceptance, board closure and worktree
   release remain distinct: accept after review, record the disposition, and
   release reviewed children before the Lead workspace. Completion, a
   completion message or `/agents` navigation establishes none of those steps.

Keep one native CLI. `/agents` is the human view of the same app-server
threads; `executor_status` and `executor_wait` are the bounded runtime views.
MCP is transport, and the consuming project's board remains the durable
assignment, feedback and acceptance record.

## Orchestration configuration

Kit-owned [orchestration.toml](../global/orchestration.toml) names the root
Director route, its successor for a confirmed lead quota failure, the delegated
Lead route, the Executor profiles, the three immediate-parent limits, the
installation-wide Executor envelope, and the shared worktree pool's directory
capacity. `lead_profile` and `successor_lead_profile` remain root-only settings;
`delegated_lead_profile` selects the route for Leads dispatched by the Director
and does not inherit root succession authority. Installation check rejects a
missing profile or a non-positive or oversized limit without substituting
another route. `default` is the native session with no `--profile` flag. Other
names must exist as `CODEX_HOME/<name>.config.toml` or `[profiles.<name>]`.

The shipped delegated Lead route is `ds`. A configuration written before
`delegated_lead_profile` existed remains compatible: omission resolves to its
first configured Executor profile, so a custom route is not silently replaced
by `ds`; an explicit local value always wins. The shipped parent limits are 2
Director Leads, 4 Director Executors and 6 Executors per Lead; the shipped
installation-wide Executor limit is 16 and pool capacity is 18. Install and
update materialize an explicit pool setting for a configuration written before
the capacity split - including a checkout still carrying the old shipped
defaults - writing its former effective capacity before publication so later
Executor-limit changes cannot resize the pool. `worktree_limit` stays
compatibility-only.

Synthetic example (not a live consumer):

```toml
schema = 1
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

### Counting and composing limits

`director_max_leads`, `director_max_executors` and `lead_max_executors` count
managed children by immediate parent and role. A Director's Lead limit counts
only Leads directly under that Director. Its Executor limit counts only its
direct Executors, not Executors under those Leads; each delegated Lead has its
own `lead_max_executors` allowance. Grandchildren therefore do not consume a
Director's direct-Executor positions. Leads count against their Director's Lead
limit and their worktree leases, not the installation-wide Executor envelope.

Every admission count includes startup reservations, active runs and runs
waiting for a reply. Waiting for an answer does not free a position. A stopped
or terminal run releases concurrency only after its effects are reconciled;
its worktree stays held, and a completed but unaccepted result also keeps its
worktree. Explicit release, not completion, stop, archive or release of
concurrency, frees that worktree.

The immediate-parent allowance, `max_concurrent_executors` and
`worktree_pool_size` compose rather than replace one another. The parent limit
is a per-parent gate, the Executor limit is the installation-wide envelope
across parents and repositories, and pool capacity independently bounds
directory positions; actual admission also requires a free pool position.
`max_concurrent_executors` does not include Leads, while pool positions cover
both Leads and Executors. Capacity for each repository's pool comes from
`worktree_pool_size`.

The effective configured bottleneck is the smallest of the applicable parent
allowance, `max_concurrent_executors` and `worktree_pool_size`, with all equal
minima named. Configuration diagnostics report the reachable limits and this
binding constraint, so a refusal can be traced to the limit that must change.
The shipped full tree is `2 * 6 + 4 = 16` Executors plus two Leads, hence
eighteen workspaces in a single repository. Positions remain lazy and
reusable. Lower local limits are valid operating choices and never stop healthy
runs; larger role limits require a deliberate increase of the separate shared
envelope and pool capacity.

The model-free installation check reports `delegated_lead_profile`, all three
parent limits, the shared Executor and pool limits, and an effective-capacity
entry for Director and Lead Executors. Each entry names its binding
bottlenecks. A missing profile or non-positive limit is a concrete check error;
it is not replaced by another route. A configuration written before these
settings keeps its root profiles and resolves the omitted delegated route from
its existing Executor profiles. Install/update materializes only the missing
pool capacity and preserves explicit local values.

The installed `executors` plugin supplies the ordinary workflow. A Director
uses `lead_dispatch` for a prepared OpenSpec change and `executor_dispatch` for
a direct Executor; a verified delegated Lead uses `executor_dispatch` for its
own Executors. Both routes expose `executor_status`, `executor_wait`,
`executor_message`, `executor_reply`, `executor_ask_lead`, `executor_stop`,
`executor_resume` and `executor_release` with the role boundaries described
below. `executor_release` records the lead's explicit `merged`/`discarded`
disposition; completion, stop, archive or a new dispatch never frees a slot.
The plugin is one shared server and one installed command; role-specific tool
visibility is applied per session, not by separate plugin identities.
Start a fresh lead session after installing or updating the plugin so its MCP
tools and role guidance are loaded. Dispatch uses the calling thread's
repository, the installed kit configuration and a pooled checkout, and creates
the child thread on that same app-server. `executor_dispatch` accepts `base`, an
allowed `profile` and a fresh `owner`; `lead_dispatch` additionally requires
the prepared OpenSpec change and its declared planning, repository and board
inputs. Neither tool accepts a thread, endpoint, model, provider or effort as
authority.

The configured Executor profile or `delegated_lead_profile` from the example is
the complete model/effort selection for its route; an unlisted requested
profile is an error, and the tools accept no per-assignment model or effort
override. An explicit user `codex --profile <id>` keeps native precedence over
role configuration. A missing plugin/tool, unavailable endpoint, missing
profile or role input, no free slot, or fetch or startup failure blocks
dispatch with its exact cause and remedy. There is no silent fallback to
another provider, raw `codex exec`, terminal automation, in-session subagents
or a second tracker. Only an MCP- or installation-reported failure blocks
dispatch; a routing-rule interpretation, the single profile or unknown quota
does not. Quota succession looks up the root successor profile's model in the
native catalog; the verified handoff seed stays a configured binding, not a
hardcoded provider role.

Dispatch returns the run id, child thread and role, checkout, slot, fixed label
and committed base after the first turn is accepted. Inspect the child in
`/agents`;
`executor_status` is the compact machine-readable view and `executor_wait` is
the blocking result or question path. Assignments live on the beads board;
executors set `lead_review` when done instead of closing. Their immediate parent
reviews that inbox and its own assigned tasks. No separate mailbox,
per-executor console or result file is part of the workflow.

A structured assignment's brief carries the result consumer, the standing
escalation boundaries, the executor's own investigate, implement, check and
self-correct cycle, and the compact result expected back: done and remaining
work, the checkout and base worked from, files, actual checks, limitations,
required decision and detail locator. A declared `consumer` or `escalate`
trigger extends that contract instead of replacing it, and an executor resolves
ordinary implementation errors - syntax, API names, failing checks - itself,
using `executor_ask_lead` only when a trigger is actually hit. The lead replies
to the exact request through `executor_reply`.
The schema, optional fields, defaults and limits live in
[native commands](rust-native.md#structured-executor-assignments).

Every delegated Lead and Executor session runs as a single-agent worker. MCP
dispatch and continuation pass Codex CLI's built-in `agents.enabled=false`
configuration, so the child thread has no built-in agent-spawning or
agent-messaging tools even though the `ds` model catalog advertises multi-agent
v2. The installed launcher applies the same setting to every Codex process
started inside those sessions, so a raw nested `codex` invocation is still
single-agent. A delegated Lead may create only its assigned Executors through
`executor_dispatch`; an Executor must not call it. Ephemeral `spawn_agent`
helpers are root-Director-only. A child that needs another agent or executor
reports the need instead of creating one. Clearing the marker and calling the
registered upstream Codex executable directly is outside the supported entry
points; the guards are not a sandbox.

The `team-lead` skill owns lead activation, briefs, steering, acceptance and
stop. Activation comes from a user request or from the main session's own
judgement for parallelizable work that repays orchestration; ordinary
sessions without that activation spawn nothing, and an executor reports a
further delegation need to the lead instead of widening orchestration.

## Managed responsibility roles

A managed session resolves its responsibility role from the source root its
installed build recorded - [roles.toml](../global/roles.toml) and the
fragments it names in the kit checkout - through the runtime's shared role
resolver, then composes the result with the applicable native user, project,
profile and session context before any model request. `executor_dispatch`
resolves the executor role for its new thread, `lead_dispatch` resolves the
delegated-Lead role, and a managed root session resolves the Director/lead
role. The consuming checkout never needs to own kit role assets, and
native precedence is preserved: an explicit user `--profile` still wins,
retained explicit skill disables and unrelated or project skills stay untouched,
and no shared user configuration is mutated. Missing or inconsistent role
inputs refuse with their exact cause before the thread's model request; a
dispatch keeps its slot bound and recoverable instead of hosting a conversation
without its responsibility role.

The MCP dispatch record identifies the resolved role and the native threads.
`executor_resume mode=exact` continues the same thread and therefore keeps the
developer text saved at its start, carried forward unchanged across repeated
exact continuations (skills the live catalogue gained still refresh).
`executor_resume mode=fresh` creates one new bound thread, retains the
predecessor and starts from the recorded assignment; inspect its effective
context rather than claiming it loaded a newly resolved revision. Role
configuration that cannot be resolved blocks dispatch before model work.

Inspect a composition without a model call:
`codex-harness skills catalogue --role lead|delegated-lead|executor [--profile ID]`
applies the installed build's resolved role to the native skill read and
refuses when those role inputs are missing instead of reporting a set the
managed session would not see. Passing `--profile ID` selects the native
profile layer the managed session selected (the installed profile file or
its `profiles.<name>` table; `default` adds nothing), so its retained
context, including a skill that profile disables, reaches the composed
native read; the command reports
`role_context=role=...;profile=...;revision=...` and composes no model
routing. The `codex-harness check` report lists each role as `resolved` with
its revision or as a `gap` naming the exact missing input; a gap blocks
only the sessions whose role inputs are missing and fails nothing else.

### Director and delegated Lead

The root is the Director. It may implement directly, dispatch an Executor with
`executor_dispatch`, or dispatch a prepared OpenSpec change to a delegated Lead
with `lead_dispatch`. A Lead assignment needs the change's applicable planning
artifacts, exact committed inputs, repositories, board issue, objective and
testable acceptance; a title or board card alone is refused before allocation.
The delegated Lead may dispatch Executors, must accept and integrate their work,
and reports the combined result to the Director. It cannot dispatch another Lead
or activate `team-lead`. Executors report only to their immediate parent.

The Director sees the hierarchy's thread and descendant status, but inspecting a
descendant does not acknowledge the Lead's pending result, resolve its question
or accept its work. `executor_wait` remains a direct-child wait. Stop, resume and
release keep their existing exact-run meaning; subtree stop and explicit
restoration are separate operations.

### Dependencies and combined acceptance

Record a cross-block dependency on the owning board instead of inferring it
from the supervision tree:

```powershell
bd dep <prerequisite-id> --blocks <dependent-id>
```

The dependency stays visible through `bd dep list <dependent-id> --json` and
the dependent issue's `dependencies`. Independent work may proceed, but the
dependent's implementation or acceptance waits until the prerequisite contract
is verified and that result is recorded on the board. If `executor accept`
finds a live `blocks` dependency, it names the prerequisite and refuses before
writing acceptance metadata or closing anything; the dependent stays open and
the same retry proceeds after the prerequisite is accepted or otherwise
verified and closed. Do not bypass the wait with `--force` or a manual accepted
close.

Accept each Executor result into its Lead, then accept the Lead's integrated
result as the Director. Direct Executor work is accepted by the Director. A
Lead block with unfinished board children is refused before effects, so close
Executor work before accepting the Lead. When individually passing outputs
fail their combined user path, create an open correction task assigned to the
owning Lead or Executor, record the failing command and evidence, and leave the
parent open. Accepted child results and exact revisions remain available; the
Director does not claim the combined outcome until that correction is accepted.

For a multi-repository outcome, acceptance records the exact clean revision of
each included checkout. If one repository integrates and another fails, report
the accepted repository and revision separately from the open failed repository
and its retained result, then keep combined acceptance pending. A Git merge
alone does not establish behavioral compatibility. Repeating an already
successful acceptance returns the recorded result with `duplicate: true` and
makes no second closure effect.

## Executor utilization

While the lead role is active, substantial eligible investigation, writing or
code is dispatched to available executor capacity before the lead starts it,
and released capacity is backfilled before unrelated lead work starts. An idle
capacity's cause stays identifiable from the board and `executor_status`: no
checkpoint ritual, reason note or occupancy narration is required, and a
summary that would otherwise mislead about capacity that sat unused names the
cause in a clause. Requirements live in the
[delegation](../openspec/specs/agent-delegation/spec.md) and
[orchestration](../openspec/specs/lead-agent-orchestration/spec.md)
specifications, and the priority rules are owned by the
[`team-lead` skill](../.agents/skills/team-lead/SKILL.md) rather than restated
here.

## Instruction-refresh succession

When an accepted instruction or skill change must reach an active executor
assignment, do not steer the stale thread as if it had reloaded the change.
Finish or stop it at a safe boundary, commit the required inputs, and dispatch a
new run with `executor_dispatch` from that base. For an interrupted assignment
whose partial work must survive, `executor_resume mode=fresh` preserves the
predecessor and starts from the recorded assignment without resetting the slot.
Inspect the new thread's loaded context and actual workspace before relying on
the refresh; an unavailable reload is reported rather than inferred. Same-session
compact recovery, catalogue delivery and in-process activation remain owned by
`autonomous-skill-evolution`.

## Board workflow

Asynchronous assignment and executor feedback use the consuming project's
`beads` (`bd`) CLI. Kit install delivers pinned v1.3.0 onto `harness/bin` with
`--board-only` and the `board-workflow` skill through core skill linking. The
controller does not parse the board. Missing or broken board state is an
explicit limitation, not a substitute tracker. Init is
`bd init --skip-agents --non-interactive --quiet`. Stages are epics,
specifications are features, executor feedback is a `task` labeled `feedback`,
and `bd status` / `bd list --label feedback` / `bd epic status` are the
non-interactive reports. Do not run `bd setup codex` from kit install.

## Improvement loop

Feedback is a board queue, not chat: lead and executor observations become
bounded `feedback` tasks, batch-triaged by the lead at safe boundaries.
Listing, merging, voting and promotion are `bd` commands; only the lead's
similarity and consequence judgments are model work. The incubator holds unique
items, a merge or repeated report adds exactly one vote per distinct episode and
reporter, and `vote_threshold` (kit `global/orchestration.toml`, default 3)
promotes an item in routing order with kit-level wording only. A material
correctness, integrity or safety finding promotes immediately under the lead's
consequence override with the reason recorded. The lead sweeps the incubator
when it closes a stage or epic and when a triage batch finds it above
`incubator_size_cap`, archiving stale items with visible reasons instead of
deleting evidence. The `board-workflow` skill owns the record formats; the
`team-lead` skill owns the workflow. The evidence-driven sequential experiment
loop is a separate mechanism with its own hypotheses, candidate selection and
removal gates: use the `self-improvement-loop` skill and the
[`codex-harness improve` controller](rust-native.md#improvement-controller);
it counts no incubator votes and does not change this promotion policy.

Pacing uses three scoped sources only: the native Codex/GPT limit snapshot the
CLI records for itself, actual provider refusals, and bounded dashboard
snapshots the user supplies. Unknown stays unknown - no probe call and no local
request-count remainder - and unknown holds the configured limits. Below 70%
used keeps configured concurrency and cadence, 70% or more halves new
concurrency and the triage batch (at least 1), and 90% or more, or an observed
refusal, waits for the reset with concurrency 1 and a `low` effort ceiling.
Pacing changes new assignments only: a healthy executor keeps its slot, model
and instructions, and tasks released by one reset are spread by a 120-second
stagger instead of bursting together. Decisions are recorded on the board with
reason, basis and expiry, and withdrawn with a revoke record.

No improvement becomes a default for assignments, worktrees, concurrency or
cadence before a matched comparison declares its tolerance in advance and shows
unchanged-or-better quality inside that tolerance, with check time, coordination
and rework counted in both arms. The `codex-harness feedback ledger` view keeps
the recorded decision and the comparison it supports separate: an adoption is
supported only when the recorded comment names both arms, a positive matched
count, finite tolerance and timing values, regression arithmetic consistent with
the recorded arm seconds and delivery inside the declared tolerance. Missing,
contradictory or malformed records stay visible with their limitations, and the
newest record naming the item controls the status - a newer unreadable record
never silently restores an older adoption. These checks read the recorded board
comment; they do not rerun the comparison or certify it independently. An
inconclusive or rejected comparison, or no record at all, leaves the improvement
unadopted, and a user-accepted trade-off remains a recorded decision rather than
demonstrated improvement. The gate evaluates orchestration defaults; it is not
the skill-evaluation contract for library mutations.

## Specifications across repositories

Choose one specification home for a change that crosses repositories: the
repository holding the OpenSpec change, its tasks and the coordinating board
record. Link the other selected code repositories from the assignment rather
than copying the change or board. A repository not selected there is outside the
assignment, even when it contains related code.

Before dispatch, name the exact committed revision of every required input and
the exact integration target in each repository. Children start from those
commit snapshots and report the bases they actually used. Do not copy uncommitted
parent files into a running checkout, substitute an unrelated default branch, or
reset work when an input moves. If a required input changes, keep the running
base and partial result until an explicit continuation or re-dispatch names the
new committed input.

Each selected repository gets its own distinct, owned integration worktree, and
a delegated Lead receives all selected roots. Keep those roots explicit in the
assignment and board record; do not copy a checkout, create a nested
repository, or write into another Lead's position. Executor dispatch still
allocates one primary worktree for its child; an Executor that needs another
selected repository must report the missing binding rather than writing into
the Lead's workspace. Two Leads may use one repository, but their worktrees,
owners and concurrency remain independent under the shared pool. Acceptance
records a revision vector for the selected repositories and states any
repository left unintegrated. Exercise the combined user path from the
integrated worktrees; a conflict-free merge does not prove that path.

Preparation completes for every selected repository before model work starts. A
failure identifies the repository, input or base that failed, the workspaces and
reservations already prepared, and the action needed to retry; unused new
reservations may be released, while pre-existing work and uncertain effects stay
preserved. Do not start a partial model run, reset an occupied position, or
describe a failed preparation as an empty assignment.

## Executor worktrees

`executor_dispatch` and the ordinary `codex-harness worktree` operations allocate
isolation through one durable task owner: one pool per repository, shared by
executor threads and ordinary tasks so neither can bypass or overwrite the
other's held position. Slots are sibling directories
`<repository-name>-<label>` bounded by `worktree_pool_size` (the legacy
executor-derived capacity when the configuration omits it). The fixed hierarchy
labels occupy these positions in every repository pool:

| Positions | Label | Owner |
| --- | --- | --- |
| 1-4 | `de1..de4` | Direct Director Executors |
| 5-6 | `l1..l2` | Leads |
| 7-12 | `l1e1..l1e6` | Executors under Lead 1 |
| 13-18 | `l2e1..l2e6` | Executors under Lead 2 |

The ordinal is the label's position in its role range, not an authority claim.
MCP hierarchy dispatch selects the first free label in the caller's range and
uses that same label in every repository selected by the assignment. A
multi-repository Executor therefore receives its own worktree in every
repository, for example `<repo-a>-l1e2` and `<repo-b>-l1e2`; it never writes in
the Lead's `l1` or `l2` checkouts. Slot records and receipts carry both the
numeric position and this label.

Compatibility CLI and ordinary task allocation keep the legacy
`<repository-name>-wt1..wtN` names until a position is migrated. Both names are
recognized as the same position, and a held or active legacy worktree is never
moved. Hierarchy reuse migrates a clean released legacy position to its fixed
label only after the pool has proved it has no owner, local work or commits
beyond its synchronized base. A duplicate alias, a foreign labeled path, or a
legacy checkout with local work is refused with the exact path; release or
merge that work before retrying.

A legacy Lead receipt with no recorded label keeps the compatibility `l1`
executor range; its worktrees remain readable, and the separate `l2` range is
never inferred from a legacy position.

MCP dispatch resolves the installed kit/configuration source and the calling
thread's target repositories; the targets need only be Git checkouts, not kit
configurations of their own.

Managed ordinary, lead and executor sessions suppress the competing native `codex_worktrees` tool surface through the shared configuration and the forwarded `features.worktrees=false` leaf; [native commands](rust-native.md#native-worktree-tool-suppression) owns the precedence rules, the conflict report and the model-free catalogue checks. A conversation that was already running when the suppression arrived keeps the tools it registered, and no in-place update exists: end it and start a fresh session through the normal route. A higher-precedence explicit `features.worktrees=true` keeps the tools; the managed start reports that configuration conflict with its removal remedy rather than successful managed-pool delivery.

A fresh hierarchy dispatch claims the first free label in the caller's fixed range and binds it to the owner as a durable task with its exact committed base before the first model request; compatibility CLI and ordinary tasks continue to walk positions with legacy naming. The claim uses the refreshed upstream default branch by default, or the explicit `base` commit resolved locally without a fetch. The claim stays exclusively owned and a competing dispatch cannot reclaim the position or replace its pending assignment. Two free positions still start independently. Dispatch is also admitted against `max_concurrent_executors`, independently of the pool: free positions alone do not grant another active executor run, and a held or suspended task keeps its position without consuming concurrency. A confirmed-dead clean unused reservation can be recovered, but a durable task binding is never freed by process exit alone and a former owner cannot continue a reassigned task. Ad-hoc worktrees are refused, and a position holding unreviewed work is preserved rather than reset.

A fresh preparation fetches the configured remote once before the claim and resets the position to the resolved base, removing untracked files while retaining ignored caches such as `cache/`; a fetch failure aborts before any mutation, and a continued binding is never fetched or reset. MCP completion does not release a slot. The lead merges accepted work, then explicitly records `merged` or `discarded` with its reason through the owning pool lifecycle. Release freezes the observed generation of the exact binding, so a superseded release refuses instead of resetting whatever holds the position later. It resets only a clean, reviewed position and retains ignored caches. Generated `.serena/` project state is tool state: release preserves it and excludes it from the cleanliness decision, while any other local or untracked work still preserves the position. A live owner is never released or reset. A position that cannot reset safely is preserved with its reason rather than reported free.

A missing host lease does not mean a position is free while its creator is
starting. If a competing dispatch refuses, inspect its reported cause and
`executor_status`; do not release or reset the position to bypass startup
contention. An uncertain process identity preserves the reservation until its
owner is resolved. A failed launch keeps its original error, and only a
confirmed-dead, clean unused reservation can be recovered automatically.

`executor_release` refuses a delegated Lead while any child run under it is
unreleased, naming the remaining runs. Review and release each child through its
own parent first, then release the Lead workspace. Acceptance, completion or a
stopped parent never authorizes resetting a child's workspace.

Ordinary tasks use the same pool without a conversation:

```powershell
codex-harness worktree acquire --task ID [--repository REPO] [--base REV]
codex-harness worktree continue --task ID [--handle HANDLE]
codex-harness worktree list
codex-harness worktree release --task ID --disposition merged|discarded --reason TEXT [--handle HANDLE] [--base REV]
```

Acquire resolves the exact local base without fetching and prints the pool line with the slot receipt (slot, path, presence, state, base, task, handle, owner, disposition, reason); repeating it for a held task returns the same binding, and a different requested base refuses as a conflict. Continue returns the held binding; a stale generation handle, a released binding or a live foreign session refuses with its exact cause. List reports every configured position plus registered-missing and foreign directories for lead review.

`executor_status` is the authoritative run and slot view for MCP executor threads,
including each run's base, state and pending request. `codex-harness worktree list`
remains the inventory for ordinary task bindings and reports registered-missing
or foreign directories for lead review. `git worktree list` remains the tree
inventory; `worktree_limit` is compatibility only. Legacy task-named worktrees
are neither adopted nor deleted automatically: preserve their work, merge or
record a discard, then remove them only by an authorized explicit Git operation.

An older harness reports upgraded pool records as unreadable with a missing
`schema` field, and the current reader refuses an `ownershipSchema` value it
does not support. Both are compatibility refusals that protect held hierarchy
work during rollback. Keep the records and worktrees, including released
positions, and restore a compatible harness through the installation lifecycle
before using that pool. Deleting records or rewriting their discriminator can
expose held tasks to the older host's expired-session cleanup. The current
reader accepts supported legacy records without rewriting them during
inspection; an authorized state transition writes the protected format.

Reducing `worktree_pool_size` below a held position refuses new acquisition;
status and continuation keep the held task visible and recoverable. Restore
the previous capacity or finish and explicitly release those tasks before
acquiring more work. A failed release leaves its binding held with the cause;
resolve that cause and retry the release instead of deleting its record.

Retiring a pool directory is separate from reusable release. Preserve and
integrate its result (or explicitly discard it), release the task through the
harness, then use `git worktree remove -- PATH` for that exact listed directory
when its ignored caches are no longer needed. Keep the released ownership
record. A later acquisition can recreate the same configured position; it
does not allocate a replacement outside capacity. For a registered but missing
directory, first resolve the preserved work and verify the path is really
absent, then follow the reported `git worktree prune` remedy. A held binding
still requires its own recovery; pruning is not task release.

## Observed executor lifecycle

`executor_dispatch` starts one executor thread on the calling lead's app-server
and records its run, profile, checkout, base, slot and thread identity. The
thread is visible in `/agents` with its title, conversation and state. There is
no per-executor terminal tab, console host or relay in the ordinary workflow;
opening or closing the native view is not a lifecycle event.

Resuming the same lead thread on a new app-server does not create a new run.
On the next authorized MCP operation, an ended recorded app-server process is
reconciled with the live registered endpoint only when that endpoint still
names the same lead thread. The receipt's process identity is updated under its
own lock and retains a bounded `appServerRebind` record; the run, executor
thread, slot, checkout, base and profile stay unchanged. If the old process is
still live or its end cannot be proved, the operation fails before native work
because two runtimes must not control one run. Do not edit the receipt by hand
to bypass this refusal.

`executor_status` reports the bounded state and pending questions for runs owned
by the verified calling thread. `executor_wait` blocks across the lead's
eligible unreleased runs by default and returns immediately for an actionable
question, otherwise returning current unacknowledged terminal results, an idle
state, a stable timeout, or an explicit observation failure. Supply `run` to
target one verified run; that read still returns its result after
acknowledgement. The default and maximum bound is 1800 seconds
(`1800000` ms): a timeout means work is still pending, not failed or stopped,
and needs no model-side renewal loop. Pass result ids from a received response
in `acknowledge` on the next call; acknowledgement is retry-safe receipt only,
does not accept work, release a slot or resolve a question, and acknowledged
results stop waking default all-run waits. A targeted response carries `result`;
an all-run response carries `results` and `pendingRequests` with each entry's
`run` and `resultId` or `requestId`. It starts no model turn and needs no picker
navigation. Use one pending wait and resume that call after a short tool yield
instead of polling process ids, rollout files or window pixels.

A completed native turn with a nonempty final assistant message is the executor
report. A completed turn with empty or missing output is an output defect, not
quota exhaustion. A wait timeout, quiet stream or view navigation proves neither
success nor failure. The result is not verified acceptance and does not merge,
release or close the board task.

Compatibility CLI lifecycle commands remain available for existing callers and
their regression checks, but they are not the ordinary agent path. Active
guidance uses the MCP tools; a missing plugin reports a capability failure
rather than a terminal or file-relay fallback.

## Steering and stopping executors

`executor_message` sends one literal lead-to-executor message over the native
conversation. Address the `run` from `executor_status`, supply `text`, and reuse
`clientMessageId` only for an idempotent retry of the same content. For an
executor question, answer the exact reference with `executor_reply requestId=...
text=...`; a reply resolves that request and continues the same thread without
resuming or reconstructing an address. Message only to add a fact, resolve a
request or correct an established mistake - not for status, hurry or repetition
without new facts. A local success or queued delivery is not evidence that the
recipient applied it.

An executor's only escalation channel is `executor_ask_lead`, which addresses
its originating lead and returns a stable request id without waiting for the
answer. The lead sees it through `executor_wait` or `executor_status`. An
unanswered request keeps the thread, slot, worktree and partial work live; it is
not completion or a reason to resume. The executor continues independent work
and does not poll.

`executor_stop` interrupts only the addressed run and its recorded background
commands. For a delegated Lead it stops the verified subtree: it closes new
child admission, interrupts the Lead and its recorded Executors, reconciles
uncertain starts, and leaves sibling teams and direct Executors running. It
preserves every affected checkout, slot and partial result and reports whether
containment is complete. Stop only for explicit cancellation or a concrete
necessity: demonstrated wrong direction, inability to progress, or a resource
conflict the executor cannot resolve. Waiting, silence, slowness and timeout are
not causes. A stopped Lead subtree remains preserved when capacity returns;
stop never restarts it, releases its slots or authorizes a replacement dispatch.

`executor_resume mode=exact` continues that same thread with the preserved
assignment and bounded progress; it does not resubmit the assignment, reset the
slot or repeat completed work blindly. It also clears a stopped Lead subtree
only through that explicit continuation; recorded children are not restarted.
`mode=fresh` starts one new bound thread, retains the predecessor, restores the
recorded role, parent, workspace set and profile binding, and requires
inspecting interrupted effects first; a fresh Lead successor reattaches
surviving children without duplicating them. Partial containment refuses
continuation until the owned work is verified gone or preserved for inspection.
A stopped run is never restarted automatically, and capacity recovery alone
never justifies a duplicate dispatch.

## Idempotent dispatch and admission

`executor_dispatch` and `lead_dispatch` accept an optional `requestId`, scoped
to the verified parent. When it is omitted, the id derives from the role,
parent, supplied owner, base, profile and complete assignment. Repeating the
same id with the same intent returns the original receipt with
`reconciled: true` and never allocates another child. Reusing the id with a
different intent fails before allocation; use a new id for a changed
assignment.

Preparation records live under
`CODEX_HOME/harness/executor-pool/requests/<parent>/<requestId>.json`. A
`prepared` record whose owning process is gone is retryable. `starting`,
`thread-started` and `uncertain` records are preserved evidence: inspect or
explicitly stop, resume or release the recorded child, and do not dispatch a
duplicate. A lost start response therefore retains one child and its pool
claim for reconciliation.

Admission counts active preparation records and unreleased receipts by the
immediate parent and role, including startup reservations, active turns and
reply-waiting runs. `director_max_leads`, `director_max_executors` and
`lead_max_executors` compose with the installation-wide
`max_concurrent_executors` and the independent `worktree_pool_size`. A lower
limit blocks new starts and leaves already admitted work running. Release
remains the explicit reconciliation gate; a stopped or terminal run keeps its
worktree until that release.

## Provider-neutral cache-loss protection and recovery

An MCP executor run monitors its exact native conversation's per-response
input/cache counters. Support is proven at runtime: a recorded response with at
least 100,000 input tokens and 90% cached input warms that run. Provider
branding or numeric-but-never-warmed counters prove nothing. After warmup,
three consecutive distinct responses each missing at least 100,000 tokens and
50% of input trigger the repeated-loss guard. Cold starts, duplicate
observations, below-threshold responses that reset the count and historical
losses on continuation do not count. Usage arrives after billing, so an
in-flight request can still cost money; this is a repeated-loss guard, not a
currency cap or an upstream-cache repair.

The run status reports missing, invalid or never-proven counters as unavailable
coverage, with the resolved provider and model. Cache loss is a terminal
non-success outcome, not a dismissible warning or evidence against the model
approach. The lead reviews preserved commits, uncommitted/untracked files and
saved test/build evidence before acting.

For recovery, use `executor_resume mode=fresh` with bounded progress. It starts
one new bound thread, retains the predecessor and its checkout and runs from
the recorded assignment; it does not fetch, reset or clean the slot, and it does
not replay the assignment as if the interrupted effects were fresh. The new
thread must inspect the actual diff, commits and interrupted tests/builds before
reusing any prior result. A partial stop must be resolved or preserved first.
Investigate recurring loss instead of creating an automatic restart loop.

## How selection works

Kit executor assignments go to the configured executor profile - currently
the single `ds` profile (DeepSeek V4.1-Flash, `max`) - with no substitution:
in-session routing preferences never override it and never justify
withholding a dispatch; report a wording conflict and dispatch anyway. A
short edit,
tightly coupled slice or expensive context handoff is often cheaper to do
directly. Count briefing, execution, waiting, checking, integration and rework.
The number of children is not a savings metric.

Before dispatch, perform the analysis each slice needs to become sufficiently
specified for its executor profile - requirement interpretation, risk and
consequence decisions, approach direction and acceptance conditions - and size
slices to the configured profiles' reasoning capability. A slice too demanding
for every available profile stays with the lead, is split further, or uses a
bounded principal consultation; it is never delegated as-is. Investigation
inside a delegated slice's boundaries remains executor work.

Combine related routine into one substantial assignment and state material
input bounds up front. Routine supervision runs at most once per 15 minutes of
wall-clock time, batched across active executors, using `executor_status` and
one pending `executor_wait`. Check earlier only for a delivered result, explicit
error, help request, new or corrected user instruction or concrete risk to
correctness or shared resources. Do independent work between events; when none
remains, keep blocking on the wait instead of polling, rereading worker
source, diffs or logs, duplicating the investigation or manufacturing status
work. The interval is not a task deadline, and silence is not a stall.

That interval is not the assignment deadline. A message to a working agent must
add facts, correct an established error or change the task. An empty or
intermediate result requires checking current work; by itself it does not
establish quota exhaustion or authorize automatic GPT takeover. Reconcile the
delivery mechanism and partial result before reassignment.

After restoring a parent session, `resume_agent` can lose the previous
subscription binding. Do not recover such a session through that tool: inspect
partial work and hand it to a fresh explicitly selected agent with brief
context. An old model record does not prove a new binding, and a service-state
check error on an old conversation does not mean the route is permanently
unavailable.

The parent passes the concrete result, inputs, dependencies, change bounds,
invariants, resource ownership, meaningful check and parent consumer. Usually
`fork_context=false`: needed context, not full history. Keep this in the existing
brief or task; a separate assignment document is not required. A research-only
assignment can finish with an evidenced answer and its limits, while an
automation assignment needs a replayable result with checked preconditions,
effects and return or recovery behavior.

Two independent slices can run in parallel on non-overlapping files or separate
worktrees. Shared desktops, services, installed applications and devices need
one owner, isolated allocation or serialized interaction; worktrees isolate
files only, not credentials, services, ports or databases. Preserve aggregate
resource limits across workers. The parent does
useful independent work without repeating the investigation, then checks the
returned result, validity conditions and unresolved restoration or dependencies.

Supporting work is delivered when its intended parent consumes the result and
required integration and restoration pass. A discovery report or experimental
script leaves promotion into the existing mechanism owner and parent acceptance
pending. On interruption or reassignment, retain verified partial work, check
its current conditions and continue from that boundary. Decomposition can also
be performed directly by the parent when a worker would add overhead. The
[portable principles](../global/principles-of-work.md#speed-feedback-and-recovery)
own when to reconsider the method, including successive different failures in
one unlearned layer.

If an assigned model is missing from the catalogue or an access, model or quota
error is returned, the parent briefly reports the cause and continues on the
configured route; it neither substitutes a provider silently nor falls back to
xAI/Z.AI. A retry is justified by changed circumstances; do not recheck the
same error on every step. Inspect saved partial work before reassignment. There
is no mandatory escalation ladder.

## Global connection and limits

The [harness profile](../global/harness.config.toml) contains a short standing
policy and a limit of two concurrent native in-session child threads.
`executor_dispatch` creates native executor threads governed by
its immediate-parent limit, `max_concurrent_executors` and available pool
capacity in [orchestration.toml](../global/orchestration.toml). Leads use pool
positions but are not included in the installation-wide Executor count. Apply
each limit only to its dispatch mechanism, and report the smallest binding
constraint, as specified by the
[lead responsibility](../global/roles/lead.md#assignment-and-routing).
The [former Astra source directory](../global/agents/README.md) remains linked
at `~/.codex/agents/codex-harness` for installer compatibility and contains no
presets. OpenCodex routing, including its subscription middle, is retired;
direct native selection does not need those names. Do not create recursive
worker trees; this remains policy until
the controller enforces one active lead and the configured executor concurrency bound.

`developer_instructions` is a scalar: the selected profile replaces the same
base config.toml value rather than concatenating strings. The base file remains
and applies again without that profile. AGENTS.md instructions still load;
explicit CLI and trusted-project settings follow native precedence. Selection
policy is an instruction to the agent, not a forced scheduler. Asking to limit
research by time or tokens does not by itself create a hard limit. Thread limit
is configuration; generic-worker recursion is prohibited by policy. Timeout and Windows Job Object
for acceptance probes are provided by a separate runner.

Sources are read through direct links on a new start. An already open tool
catalogue may remain previous. Disconnecting subscriptions removes only their
link; direct native Astra selection and the main model remain. Install, check, disconnect and
recovery commands are in [subscription models](subscription-models.md).
Do not stop the proxy from a session that uses it.

## Subscriptions and auxiliary calls

All assigned OpenAI models belong to Astra. The xAI and Z.AI routes remain
installed but are not used for assigned work; the automatic vision helper is
off. If a selected model lacks a capability, that must
become a visible limit or a separate explicit assignment, not a hidden ChatGPT
quota borrow. Routing settings are owned by the native subscription lifecycle
described in [subscription models](subscription-models.md).

Paid API keys, purchases and hidden model substitution are not enabled. Astra
backup spends the same ChatGPT subscription as the parent. An unknown remaining
quota is not zero. Tokens help compare runs but do not exactly convert a weekly
limit: [Codex accounts for model, context, reasoning and tools](https://learn.chatgpt.com/docs/pricing).

For remaining ChatGPT quota use ordinary Codex account status; the app-server
contract is [`account/rateLimits/read`](https://learn.chatgpt.com/docs/app-server).
Keep observation time and limit scope: a snapshot belongs to the account, not
only the current task. For Grok, use available Grok-subscription limit
information and actual exhaustion responses; a reliable weekly-quota remaining
endpoint for this OAuth integration is not confirmed.
[Grok FAQ](https://docs.x.ai/grok/faq) describes subscription limits.
Local `~/.opencodex/usage.jsonl` reflects served requests, not a guaranteed
subscription remainder. Do not mix those values with Codex cumulative counters
without correlation and duplicate removal. Unknown remainder does not require
preliminary model requests before every delegation.

## Checks and replacing a model

Model-free checks are native: the delegation usage, outcome and consumer
suites under `crates/codex-harness/tests/` (see
[native checks](rust-native.md)). Isolated lifecycle uses separate homes, port
and Windows task. Legacy model probes are opt-in and capture private logs, but
do not yet open all active conversations simultaneously. Do not run them as the
new workflow's acceptance until their visible-view integration is implemented:

```powershell
& (uv python find) tests/agent-delegation.py --run-model-probes --scenario all
./tests/subscription-consumer.Tests.ps1 -RunModelProbes -GrokModel xai/grok-4.7 -GrokReasoningEffort xhigh -Scenario Delegation
```

The first probe compares the same tasks for direct Astra and two Grok children,
checks the result independently, and separately checks Astra-level bindings. One
short max consultation in it is a configuration check, not a production
threshold. Private evidence stays in a temporary directory.

### Installed orchestration acceptance

Use the installed-lifecycle command in
[installation](installation.md#installed-orchestration-acceptance) for a fresh
external consumer. Its coverage split is deliberate:

- `orchestration_isolated_install` exercises the installed launcher and MCP
  server, the three role contexts and catalogues, both route tool declarations,
  absent/forged caller refusal, the 2/4/6 configured limits with 16/18 shared
  capacity, board scope, two repository pools and explicit release with a stale
  generation.
- `executor_mcp` exercises both dispatch routes, parent/root/role identity,
  per-role `enabled_tools`, the full admission topology, stop, exact/fresh
  resume, release and reopen after disconnection with a deterministic
  app-server.
- `executor_assignment` exercises specification-backed Lead admission, board
  ownership across repositories and multi-repository assignment inputs.
- `worktree_pool` exercises shared pools, multi-repository workspace sets,
  concurrent admission and safe reset or preservation.

The installed target starts no model turn. It therefore does not establish real
`ds` delivery, live `/agents` conversations, or elevated Windows token state;
those remain separate acceptance and must not be inferred from the isolated
receipt.

The native usage counter `codex-harness delegation-usage` takes explicit parent and
child paths, keeps the last cumulative record of each thread for former
consumers, and separately removes repeats by stable response ID. Inherited
history can overlap cumulative sums; with incomplete data or series disagreement
the reconciled total is unknown. Missing, conflicting and unattributable data
are marked. The counter does not load full journals into the main-agent context.
Including an estimate in every working task is unnecessary. `null` means no
data; a provider total of zero with `thread_count=0` means none of its threads
were among the supplied files, not a zero remaining quota. The `partial` flag
conservatively spreads to aggregates on warnings. A failed run keeps available
spend data and the acceptance-failure cause.

The counter is available from any directory through the installed native
manager. Pass only needed
existing rollout files; keep results and detailed logs outside Git:

```powershell
$usageHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }
$usageState = Get-Content (Join-Path $usageHome 'harness/installation.json') -Raw | ConvertFrom-Json
$rolloutPaths = @('<parent-rollout.jsonl>', '<child-rollout.jsonl>')
& $usageState.configBridge delegation-usage @rolloutPaths --format markdown --output (Join-Path $env:TEMP 'usage.md')
```

A previous saved model probe can also be rechecked without spending the
subscription:

```powershell
& (uv python find) -B tests/agent-delegation.py --revalidate '<private-evidence-path>'
```

To replace a provider, select its enabled model and supported effort directly,
and update subscription configuration and exact-model checks where needed after
confirming the authorized catalogue and capabilities. OpenAI variants must remain in the
Astra family.

After a main conversation has started, `/btw` and `/side` open a side question.
The ordinary host keeps reflection ownership on the main conversation. Correlated
ephemeral forks and native temporary conversations remain usable while journal
recovery is pending; they do not reconcile or discard that recovery or retained work.
Return with the TUI prompt (`Ctrl+C`). Nested side chats and review mode are
unsupported. On CLI 0.153.4 both names work with `multi_agent_v2=false`. Side
chats do not isolate files; use a Git worktree for independent edits.
