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
