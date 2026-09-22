## Why

An interrupted pooled executor could not be resumed through kit commands:
`executor spawn` always resynchronizes its slot (`git reset --hard` plus
`git clean -fd`) and cannot address an exact session id. Leads worked around
this by hand-editing a dispatch receipt and calling `executor run --file`
directly. That path skips slot allocation, and when the dead session's slot
record had been reconciled back to an unowned state, the tab host's ownership
guard failed deterministically (`slot N is bound to session no session
instead of X`, exit code 2) on every restart. The failure repeated across
consumer projects until the user reported it.

## What Changes

- Add `codex-harness executor resume`: one exact session id plus one explicit
  pool slot and owner id. It rebinds that slot without fetch, reset or clean
  (partial work stays), requires the recorded synchronized base, refuses a
  live owner or another owner's claim, records the lease through the existing
  lifecycle and opens the same visible terminal or console hosts as `spawn`.
- Child arguments use the verified non-interactive resume order already used
  by instruction-refresh succession: profile flags, `exec
  --skip-git-repo-check -C <slot> resume <SESSION_ID> <PROMPT>`.
- Make the tab host's ownership failure actionable: when the slot record has
  no owner, the error names the pooled spawn/resume remedy instead of only the
  mismatch, so a hand-edited receipt is diagnosable without source inspection.
- Route documentation and the live team-lead skill for interrupted executors
  through the new command; hand-edited receipts are no longer the workaround.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `agent-delegation`: interrupted pooled executor sessions resume through the
  kit's pooled command on their own slot without resetting partial work;
  hand-edited receipts are diagnosed with an actionable remedy.

## Impact

- `crates/harness-core/src/task_worktree.rs` (slot adoption without reset,
  public upstream lookup), `crates/codex-harness/src/executor_cli.rs`
  (resume subcommand, shared launch path, ownership error remedy).
- `docs/agent-delegation.md` and `.agents/skills/team-lead/SKILL.md`
  (interrupted-executor continuation wording).
- No configuration, receipt schema or pool layout change; existing receipts,
  slot records and leases stay compatible.
