## Context

Stage 1 (`orchestrate-subscription-agents`) delivers the lead/executor loop:
configured profiles, visible windows, worktrees, board coordination, steering,
acceptance and durable recovery. This follow-up makes the loop
self-improving within an honest budget. It depends on Stage 1's board
workflow, controller state and succession machinery; it extends them rather
than replacing them.

Confirmed user decisions (2026-09-16): feedback in both directions is
recorded as triage tasks, not chat; improvement tasks are placed in an
incubator rather than implemented immediately; the incubator keeps unique
items only, with similar feedback adding +1 vote; items with more than two
votes are promoted to the backlog for the lead or an executor to implement.
Earlier confirmed decisions carry over: succession through `codex resume`,
safe-boundary replacement after in-flight tool effects, budget honesty
(improvements must not burn tokens without real quality/speed gains), and
Edge dashboard tabs as a user-visible spend source.

Accepted with the draft (2026-09-16): one vote per distinct episode/reporter
with provenance; lead consequence override without votes for material
correctness, integrity or safety risks; promotion routing small-to-backlog,
material-to-OpenSpec and kit-concerns-to-kit-backlog. Hygiene ownership was
challenged as unclear and resolved the same day: the lead owns the sweep, and
it fires on deterministic triggers - stage or epic closure during acceptance,
or a triage batch finding the incubator above its configured size cap -
instead of an unspecified periodic cadence.

## Goals / Non-Goals

**Goals:** Durable deduplicated demand signals; vote-based promotion with a
consequence override; bounded feedback cadence; instruction-refresh
succession with a verified `codex resume` contract; honest pacing from
scoped observations; a benefit gate before improvements become defaults.

**Non-Goals:** Changing Stage 1 dispatch, visibility or acceptance behavior;
the mechanics of evolving skills and instructions, skill-evaluation of
library mutations, catalogue admission, in-process catalogue delivery and
same-session compact recovery (owned by `autonomous-skill-evolution`);
writing skill packages from incubator promotion; using incubator votes as a
substitute for a verified owned procedure that already belongs to
skill-evolution; autonomous promotion of instructions without the applicable
workflow; provider quota scraping with credentials or invented percentages;
guaranteed throughput claims.

## Decisions

### 1. Feedback is a queue, not a conversation

Every improvement observation becomes a bounded board task. The lead
batch-triages at safe boundaries. This keeps per-message token cost near
zero for executors, preserves evidence durably, and matches Stage 1's
board-is-the-agents'-protocol decision. Steering remains the live channel
for course correction; the incubator carries durable demand.

### 2. Votes measure demand frequency, with a consequence override

Votes are a cheap, honest signal of recurring friction. To keep them honest:
one vote per distinct episode and reporter, visible provenance, merges
decided by the lead during triage, no inflation from automated diagnostics
or self-repeats. Because frequency is not value, the lead may promote
immediately on material correctness, integrity or safety evidence with a
recorded reason. The threshold is kit configuration with a default of
promotion after more than two votes.

### 3. Promotion routes by consequence, not by habit

Small improvements become backlog tasks; behavior or requirement changes
must enter OpenSpec; items about kit instructions, skills or tools promote to
the kit's own backlog so the signal aggregates across consuming projects
without leaking private data. Promotion confers eligibility for planning,
never silent implementation authority.

### 4. Observations are routed by kind, not double-counted

The same friction must not become both an incubator vote and an immediate
skill candidate. A verified reusable procedure in owned skill scope is
handed to `autonomous-skill-evolution` and does not wait for three votes.
Process, orchestration, requirement, tool and unclear or material changes
stay in the incubator. Kit-wide skill or instruction demand from consuming
projects promotes to the kit backlog without private data; skill-evolution
then executes any library mutation under its own evaluation contract.
Promotion never writes `SKILL.md`.

### 5. Hygiene is lead-owned with deterministic triggers

An unreviewed incubator becomes a graveyard that erodes the signal, but a
"periodic review" leaves who and when unclear. The sweep belongs to the lead
session and fires on two conditions the lead already observes: closing a
stage or epic during acceptance, and a feedback triage batch finding the
incubator above its configured size cap (a cheap non-interactive board query;
the archive decision itself is judgment). No background scheduler and no
controller board parsing - Stage 1 keeps the controller board-agnostic. When
no lead session is active, hygiene simply waits; archived items are
restorable, so deferral is safe. The sweep archives stale items with visible
reasons and never deletes evidence.

### 6. Instruction refresh through verified `codex resume`

A changed-instructions session spawns a successor via a deterministic
`codex resume` invocation of its profile that selects the exact session
(session id or equivalent non-interactive selection - an interactive picker
cannot be controller-driven). The handover happens at a safe boundary after
in-flight tool effects; the predecessor hands over durable records and stops
its own process. Two contracts require verification before dependence:
non-interactive session selection, and that a resumed process reloads
current AGENTS instructions and skills. This path is process succession for
orchestrated workers. It consumes the compact revision identity published by
skill-evolution (`name`, canonical path, revision, operation). It does not
implement catalogue injection, in-process activation or same-session compact
recovery; those remain owning blockers of `autonomous-skill-evolution` when
unsupported. Replacing a worker process does not close that same-session
requirement. Resume-contract verification here is a consumer check, not a
second catalogue-delivery research track.

### 7. Pacing from scoped observations only

GPT limit reads use the installed native contract where exposed. Other
providers pace from actual refusals and bounded user-supplied dashboard
snapshots (the pinned browser tabs); unknown remains unknown. No probe
requests, no invented percentages, no preemption of healthy workers. Pacing
touches concurrency, effort and feedback cadence - the triage cadence itself
is paced so the improvement loop cannot outrun the work it improves.

### 8. Benefit gate before defaults

A promoted improvement becomes a default only after a matched comparison
shows unchanged-or-better quality and no material delivery-time regression
beyond tolerance, with feedback-triage, coordination and rework included in
the accounting. This is the enforcement point for the user's requirement
that improvements must strongly justify their token burn. It evaluates
orchestration defaults, not skill-library mutations; it does not reuse or
replace the skill-evaluation accept/reject/inconclusive contract.

## Risks / Trade-offs

- Vote gaming or fragmentation -> episode/reporter-scoped votes with visible provenance and lead-owned merge decisions.
- Incubator noise -> hygiene cadence, archiving with reasons, consequence override for rare-but-severe items.
- `codex resume` contract drift -> verification tasks bound activation to the tested CLI behavior; gaps reported as "succession not established". Catalogue compact-recovery research stays in `autonomous-skill-evolution`.
- Dashboard snapshots are manual and stale -> they pace, they do not measure; reset windows and concurrency remain explicit.
- Benefit comparisons cost tokens themselves -> reuse Stage 1 evidence, bounded scenarios, stop on sufficient evidence.
- Sequencing -> this change cannot complete before Stage 1 delivers its dependencies; drafts may progress in parallel only on contract verification tasks.

## Migration Plan

1. Verify the two contracts first: `bd` merge/vote/promotion operations on Windows and deterministic `codex resume` selection with instruction reload.
2. Add feedback intake, triage batching and incubator mechanics with hygiene on the existing board workflow.
3. Add promotion routing and the consequence override; connect kit-concern items to the kit backlog.
4. Implement instruction-refresh succession on the Stage 1 controller path.
5. Add scoped observations and pacing; run the benefit-gate comparison on a real improvement.
6. Deliver through the kit lifecycle and update owning records; rollback removes the loop without losing archived evidence.

## Open Questions

- **Resolved 2026-09-19 (task 3.2):** the kit-level backlog is the kit
  checkout's own `bd` board, reached explicitly by the lead session
  (`bd -C <kit>`). The board's runtime state stays host-local and out of Git;
  only kit-level summary and scope cross over (no reporter, episode, project
  path or raw observation), so the publication boundary holds.
- How dashboard snapshots reach the lead: manual paste on request versus a
  bounded browser read; decision needed before pacing implementation.
