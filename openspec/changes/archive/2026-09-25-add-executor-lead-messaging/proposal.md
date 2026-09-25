## Why

Pooled executors can receive a lead's correction but cannot ask their own lead a question through an automatically addressed channel. A blocking clarification currently risks ending the executor conversation before the lead can answer, while routine synchronization must remain on the consuming project's beads board.

## What Changes

- Add `codex-harness lead message --text TEXT` and its UTF-8 `--file FILE` alternative. Spawn establishes the sender and its originating lead; the executor supplies no recipient, checkout, slot or session address.
- Restrict the command to a verified harness-spawned executor and its recorded originating lead. Reject forged, missing, stale and cross-run bindings rather than inferring an address from cwd, process names or the most recent session.
- Attach trusted sender, run, session, source, worktree and available assignment metadata, plus an opaque message reference for `codex-harness executor message --reply-to MESSAGE_ID --text TEXT`.
- Keep a request's executor conversation, visible surface and occupied worktree available while awaiting a reply, without model polling, premature completion or a separate resume command. Independent authorized work may continue. An optional notification mode does not request a reply.
- Make `executor watch` yield actionable waiting evidence with exit 3 while that run remains live; preserve its existing completion/failure/timeout meanings 0/1/2. Extend the TUI owner's base lifecycle instead of replacing its requirement block.
- Reuse official Codex session, queue, turn and event capabilities. Keep only relationship validation, metadata, reply resolution and the missing lifecycle coordination in Rust harness code; do not build another conversation service or replace native behavior with prompt rituals.
- Update generated executor instructions, CLI help, delegation guidance and the team-lead skill. Messages are exceptional clarification/escalation exchanges; bd remains the durable source for assignments, progress, blockers, decisions and results.
- Compose with `simplify-native-harness`: one native transport/event owner, no new conversation service, and no growth in the combined required instruction load after removing superseded escalation mechanics.
- Deliver and verify the commands through the global installation lifecycle, including an ordinary lead session and real pooled executors outside this checkout.

## Capabilities

### New Capabilities

None. Extend the existing orchestration and delegation owners.

### Modified Capabilities

- `lead-agent-orchestration`: Spawn-owned reverse addressing, native message delivery, authenticated metadata and replies, waiting-for-reply lifecycle, and reconciliation with board-first synchronization.
- `agent-delegation`: Minimal commands, executor/lead usage guidance, waiting and reply behavior, and globally delivered operation.

## Impact

The main implementation owners are `crates/codex-harness/src/executor_cli.rs`, `executor_message.rs`, `executor_control.rs`, `executor_observation.rs`, `executor_assignment.rs`, CLI command dispatch, and the existing `harness-core` native session/transport/process owners as needed. Existing dispatch receipts, endpoint records, leases and native session history remain authoritative for their respective facts. Instructions live in `.agents/skills/team-lead/SKILL.md`, `docs/agent-delegation.md`, `docs/rust-native.md` and the generated executor brief.

Existing explicitly addressed executor messaging and stop behavior remain supported. Native agent-spawning tools remain disabled in executor sessions. No new model/provider, billing route, external service, dependency or board parser is proposed. Existing launcher compatibility, visibility, process ownership and installation recovery guarantees remain acceptance constraints.
