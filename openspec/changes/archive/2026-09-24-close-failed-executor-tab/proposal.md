## Why

A failed executor tab stays open. Windows Terminal's default close-on-exit policy closes a tab only when its process exits 0, and a failed executor host exits non-zero, so the tab remains with "You can now close this terminal". That happens after the run has already recorded its outcome. One frequent cause is a completed turn whose full-thread final-message read exceeds the 1 MiB transport limit: the host then kills its child tree and exits 2, discarding a finished turn and leaving the tab behind.

## What Changes

- An executor tab opened by dispatch closes when its host process ends, including a recorded failure, without sending a terminal command and without treating that process exit as the run outcome.
- A completed turn whose full-thread final-message read exceeds the transport limit is completed from the agent message already delivered on the stream, or recorded as an output defect, instead of killing the child tree and failing the run.
- Supersedes the decision that a failed executor session stays visible in its tab for inspection. The receipt, detail file and control log remain the inspection surface.

## Capabilities

### New Capabilities

### Modified Capabilities

- `lead-agent-orchestration`: executor tabs close after the host ends, including failure; an oversized final-message read does not fail a completed turn or kill its child tree.

## Impact

- `crates/codex-harness/src/executor_cli.rs` tab command and control-host finish path
- Executor control tests and the tab-argument tests
- `docs/agent-delegation.md` and `docs/project-decisions.md`
- No terminal-settings install and no change to owned-console exit codes
