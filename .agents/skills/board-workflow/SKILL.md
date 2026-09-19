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
