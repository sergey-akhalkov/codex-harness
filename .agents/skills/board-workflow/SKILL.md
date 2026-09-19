---
name: board-workflow
description: Coordinate asynchronous lead/executor work on the consuming project's beads (`bd`) board. Use when setting up or updating epics, specification features, executor feedback tasks, or inspecting board status without a second tracker.
---

# Board workflow

The consuming project's `bd` CLI is the durable assignment and feedback record.
The harness controller does not parse it. Do not invent a replacement tracker
when `bd` is missing or broken: report `board unavailable` and continue only
work whose acceptance does not depend on the board.

Kit install delivers checksummed `bd` v1.3.0 onto `harness/bin`. Do not install
through `curl | bash`, `irm … | iex`, npm, or `go install`. Do not run
`bd setup codex` from kit install or an ordinary session; it rewrites
`AGENTS.md`, skills and hooks. Owned init is non-interactive:

```powershell
bd init --skip-agents --non-interactive --quiet
```

After init, `bd -C <project>` works. It cannot be used for `init`.

## Records

| Kind | Command |
| --- | --- |
| Stage | `bd create "…" --type epic --json` |
| Specification | `bd create "…" --type feature --parent <epic> --json` |
| Executor feedback | `bd create "…" --type task --labels feedback --parent <feature> --json` |
| Status | `bd status --json --no-activity` |
| Feedback list | `bd list --label feedback --json` |
| Epic progress | `bd epic status --json` |
| Resolve | `bd close <id> --reason "…" --json` |
| Lead inbox | `bd list --status lead_review --json` |
| Lead work | `bd list --assignee lead --json` |

`--type feedback` is rejected. Lead-initiated improvements are tasks, or
OpenSpec changes when they alter accepted requirements. Public examples stay
synthetic. Leave project `.beads/` state in the consuming project; disconnect
does not erase it.

## Bounded feedback

Lead and executor observations are tasks labeled `feedback`, not chat. Keep the
description bounded and structured:

```
observation: dispatch waits after tools
scope: dispatch
reporter: exec-a
episode: e1
kind: lead|executor|diagnostic
```

Do not paste transcripts. `kind: diagnostic` records the observation but must
not count as a vote.

## Batch triage

The lead applies merges at a safe boundary. Listing, merging and voting are
`bd` commands and make no model calls:

| Step | Command |
| --- | --- |
| List | `bd list --label feedback --status open --json --brief` |
| Admit unique item | `bd label add <id> incubator --json` then `bd label remove <id> feedback --json` |
| Merge similar | `bd duplicate <id> --of <canonical> --json` |
| Record vote | `bd comment <canonical> --json "feedback-vote v1 episode=<id> reporter=<id> kind=executor counted=true reason=counted"` |
| Record merge | `bd comment <canonical> --json "feedback-merge v1 from=<id> into=<canonical>"` |
| Inspect provenance | `bd comments <canonical> --json` |

One counted vote per distinct episode and reporter. Same-reporter repeats use
`counted=false reason=repeat`. Diagnostics use `counted=false reason=automated-diagnostic`.
Cap a batch at `feedback_batch_limit` from kit `global/orchestration.toml`.
Do not use `bd find-duplicates`; it may call a model. Similarity is lead
judgment, then these mechanics.

## Observation routing

Classify each observation at triage (lead judgment; no model call) so it is
never both an incubator vote and a skill candidate.

| Observation | Intake route |
| --- | --- |
| verified reusable procedure in owned skill scope | hand to `autonomous-skill-evolution` as a reference |
| process, orchestration, requirement, tool, unclear or material | incubator item |
| kit instruction, skill or tool demand | incubator item, then the kit backlog at promotion |

Kinds: `skill-procedure`, `process`, `orchestration`, `requirement`, `tool`,
`unclear`, `material`, `kit-concern`.

| Step | Command |
| --- | --- |
| Admit with kind | `bd label add <id> incubator --json`; `bd label remove <id> feedback --json`; `bd comment <id> --json "feedback-route v1 kind=<kind> target=incubator item=<id>"` |
| Hand off a procedure | `bd label add <id> skill-evolution --json`; `bd label remove <id> feedback --json`; `bd comment <id> --json "feedback-route v1 kind=skill-procedure target=skill-evolution item=<id>"` |

The handoff is a reference only: it writes no skill package and leaves
`SKILL.md` untouched. An item handed off is never voted into the incubator, and
an incubating item is never handed off.

## Promotion

Read `vote_threshold` and `incubator_size_cap` from kit
`global/orchestration.toml` (default `vote_threshold = 3`: promote after more
than two counted votes). Counting uses `bd comments` only.

| Step | Command |
| --- | --- |
| Eligible items | `bd list --label incubator --status open --json --brief`, then count `feedback-vote v1 ... counted=true` comments |
| Promote to backlog | `bd label remove <id> incubator --json`; `bd label add <id> backlog --json`; `bd comment <id> --json "feedback-promote v1 route=backlog-task basis=votes counted=<n> threshold=<t> target=none"` |
| Behavior or requirement change | create `openspec/changes/feedback-<id>/proposal.md`, then promote with `route=openspec-change target=openspec:feedback-<id>` |
| Kit concern | create a sanitized kit task (`bd -C <kit> create "Kit feedback: <summary>" --type task --labels kit-feedback --json`), then promote with `route=kit-backlog target=kit:<kit-id>` |

Route by consequence, never by habit: small improvements become backlog tasks,
changes to accepted behavior or requirements enter OpenSpec instead of being
implemented directly from the backlog, and items about kit instructions, skills
or tools go to the kit's own board with kit-level wording only - no reporter,
episode, project path or raw observation. Promotion keeps every vote, merge and
route comment as history and confers eligibility for planning, not
implementation authority; it never writes a skill package.

Lead consequence override: with material correctness, integrity or safety
evidence, promote without votes and record
`basis=override counted=<n> threshold=none consequence=<...> reason=<...>` in
the promotion comment.

## Incubator hygiene (lead-owned)

Two deterministic triggers, no background scheduler: the lead sweeps when it
closes a stage or epic during acceptance, and after a triage batch finds the
incubator above `incubator_size_cap`. Size checks are board queries; no model
call is made.

| Step | Command |
| --- | --- |
| Size | `bd list --label incubator --status open --json --brief` |
| Archive stale | `bd comment <id> --json "feedback-archive v1 trigger=<trigger> reason=<reason>"`; `bd close <id> --reason "archived: <reason>" --json` |
| Restore | `bd comment <id> --json "feedback-restore v1 reason=<fresh evidence>"`; `bd reopen <id> --reason "<fresh evidence>" --json` |

Triggers: `stage-or-epic-closed` and `incubator-above-cap size=<n> cap=<n>`.
Archiving keeps labels, merge history and votes, so reopening restores the item
on fresh evidence. With no lead session active the sweep waits unchanged.

## Pipeline

All agent work goes through this board. Do not use process polling or chat as
the assignment record.

Custom status `lead_review` (wip): configure with
`bd config set status.custom "lead_review:wip"`.

| Actor | Does |
| --- | --- |
| Lead | Creates epics/features/tasks, assigns executor work, records its own follow-ups as tasks assigned to `lead` so a later lead session does not repeat them |
| Executor | Claims assigned work, implements the outcome, sets `lead_review` when done. Does not `bd close` its own assignment |
| Lead | Periodically lists `lead_review` and `assignee=lead`, reviews, merges, closes or returns to `in_progress` with feedback |

On a new lead session: read the board first (`lead_review`, assignee `lead`,
ready work). Create new tasks for new situation; do not re-open completed
verification.
