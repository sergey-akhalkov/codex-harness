---
name: board-workflow
description: Coordinate asynchronous lead/executor work on the consuming project's beads (`bd`) board. Use when setting up or updating epics, specification features, executor feedback tasks, or inspecting board status without a second tracker.
---

# Board workflow

The consuming project's `bd` CLI is the durable assignment and feedback record.
`codex-harness feedback` performs deterministic bookkeeping on that same board.
Do not invent a replacement tracker
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

Lead and executor observations are tasks labeled `feedback`, not chat. Record
them through the installed command; it bounds and structures the description:

```powershell
codex-harness feedback record --project <project> --parent <feature> --observation "dispatch waits after tools" --scope dispatch --reporter exec-a --episode e1 --kind executor
```

Do not paste transcripts. `--kind diagnostic` records the observation but must
not count as a vote.

## Batch triage

The lead selects similarity and kind at a safe boundary. Inspect
`codex-harness feedback list --project <project>`, then supply a decisions file:

```json
{"schema":1,"decisions":[{"feedback":"sample-17","kind":"process","merge_into":null}]}
```

`codex-harness feedback triage --project <project> --decisions <file>` applies
the declared routing, merges and votes. Use a canonical item id in `merge_into`
for an agent-selected duplicate. `feedback ledger --project <project> --item
<id>` reads retained provenance. These commands make no model calls.

One counted vote per distinct episode and reporter. Same-reporter repeats use
`counted=false reason=repeat`. Diagnostics use `counted=false reason=automated-diagnostic`.
The command enforces `feedback_batch_limit` from kit `global/orchestration.toml`.
Partial failures report applied operations and the failed action with a nonzero
exit. Resolve the cause and retry the same decisions; recorded votes stay counted
once. Read the result instead of reconstructing this bookkeeping in prose.
Do not use `bd find-duplicates`; it may call a model. Similarity is lead
judgment, then these mechanics.

## Observation routing

Classify each observation at triage (lead judgment; no model call) so it is
never both an incubator vote and a skill candidate.

| Observation | Intake route |
| --- | --- |
| verified reusable procedure in owned skill scope | hand to `skill-evolution` as a reference |
| process, orchestration, requirement, tool, unclear or material | incubator item |
| kit instruction, skill or tool demand | incubator item, then the kit backlog at promotion |

Kinds: `skill-procedure`, `process`, `orchestration`, `requirement`, `tool`,
`unclear`, `material`, `kit-concern`.

The selected `kind` in the triage file determines these mechanical labels and
route records. Do not recreate them with separate label/comment calls.

The handoff is a reference only: it writes no skill package and leaves
`SKILL.md` untouched. An item handed off is never voted into the incubator, and
an incubating item is never handed off.

## Promotion

`codex-harness feedback candidates --project <project>` reads eligibility and
incubator size using configured `vote_threshold` and `incubator_size_cap`.
The default threshold is three distinct counted votes. Commands print the
actual limits and their source; `--source <kit>` explicitly selects another kit.

| Step | Command |
| --- | --- |
| Promote to backlog | `codex-harness feedback promote --project <project> --item <id> --route backlog-task` |
| Behavior or requirement change | `codex-harness feedback promote --project <project> --item <id> --route openspec-change` |
| Kit concern | `codex-harness feedback promote --project <project> --item <id> --route kit-backlog --kit-project <kit> --summary "kit-level summary" --scope "kit scope"` |

For the OpenSpec route, first create the intended change through the project's
OpenSpec workflow. Promotion validates/references it and preserves its artifacts;
it does not author a proposal or modify external workflows. The default name is
`feedback-<item-id>`; `--openspec-change <name>` selects an existing intended change.

Route by consequence, never by habit: small improvements become backlog tasks,
changes to accepted behavior or requirements enter OpenSpec instead of being
implemented directly from the backlog, and items about kit instructions, skills
or tools go to the kit's own board with kit-level wording only - no reporter,
episode, project path or raw observation. Promotion keeps every vote, merge and
route comment as history and confers eligibility for planning, not
implementation authority; it never writes a skill package.

Lead consequence override: with material correctness, integrity or safety
evidence, pass both `--override-consequence TEXT` and `--override-reason TEXT`.
The command records the override and preserves vote history. A completed retry
successfully confirms the recorded promotion without rewriting history; partial
retries reconcile missing labels. A different route or OpenSpec target is a
conflict. The command does not infer the consequence or authority.

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

## Scoped observations and pacing records

Pacing reads three sources and nothing else: the native Codex/GPT limit
snapshot the CLI itself records, actual provider refusals reported by
executors, and bounded dashboard snapshots the user supplies. Unknown stays
unknown - no probe call, no local request-count remainder, no invented
percentage. Record observations on the pacing item (the stage epic or a
dedicated task):

| Step | Command |
| --- | --- |
| Record | `bd comment <item> --json "pacing-observation v1 scope=<account> source=native-limit\|provider-refusal\|dashboard-snapshot used=<percent\|unknown> resets_at=<epoch\|unknown> window_minutes=<minutes\|unknown> refusals=<n> observed_at=<epoch> max_age=<seconds>"` |
| Inspect | `bd comments <item> --json` |

`used=unknown` is correct whenever the source exposes no percentage. A refusal
records the refusal only; a dashboard snapshot is an opaque user-supplied
reading and is never fetched or scraped. A native read is bounded to the
newest session records and to the provider-issued `rate_limits` object.

| Step | Command |
| --- | --- |
| Record a decision | `bd comment <item> --json "pacing-decision v1 id=<scope>:<knob> scope=<account> knob=new-assignments\|concurrency\|effort\|feedback-cadence from=<old> to=<new> expires_at=<epoch\|none> reason=<why> basis=<observation>"` |
| Withdraw one | `bd comment <item> --json "pacing-revoke v1 id=<scope>:<knob> reason=<why>"` |

A decision expires with the observation it was derived from and is re-derived
at the next boundary; there is no background scheduler.

## Benefit gate

A promoted improvement becomes a default only after a matched comparison and
only with an adopted gate record. Record the comparison on the promoted item:

| Step | Command |
| --- | --- |
| Record | `bd comment <item> --json "benefit-gate v1 item=<item> improvement=<name> outcome=<adopt\|reject\|inconclusive> quality=<unchanged\|improved\|regressed\|unmeasurable> matched=<n> tolerance_percent=<n> baseline_seconds=<n> candidate_seconds=<n> regression_percent=<n> baseline=<arm> candidate=<arm> accounting=check+coordination+rework detail=<why>"` |

Declare the tolerance before the run and include feedback-triage, coordination
and rework in each arm's seconds. An inconclusive or rejected record leaves the
improvement unadopted.

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
