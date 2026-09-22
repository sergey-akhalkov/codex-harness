## Why

Executor sessions inherit the execution profile's model catalog, and the live
`ds` catalog advertises multi-agent v2. Executors therefore received the
`collaboration` tool set and could create helper or nested executor
conversations, and they could also run the kit's own dispatch command from
their shell. Instruction-only prevention had already failed: recursive
executor trees burn quota, escape the visible-one-conversation-per-assignment
model, and break the single-agent worker contract.

## What Changes

- Executor dispatch, resume and instruction-refresh succession launch the
  session with Codex CLI's built-in `agents.enabled = false` configuration, so
  the session's tool list contains no agent-spawning or agent-messaging tools
  and no multi-agent usage instructions, whatever the selected model catalog
  advertises. The executor therefore knows from its own tool set that it has
  no agent-calling tool.
- The installed launcher applies the same configuration to any Codex process
  started while the executor environment marker is present, so a raw nested
  `codex` invocation from an executor shell is still a single-agent session.
- `executor spawn`, `resume`, `run` and `succeed` refuse to run inside an
  executor session with a concrete error that names the lead as the owner of
  further delegation; an executor reports the need instead of creating another
  executor.
- Ephemeral `spawn_agent` helpers stay a lead-only facility.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `agent-delegation`: executor sessions run as single-agent workers with the
  agent tool set disabled by the installed CLI, and executor dispatch refuses
  to originate inside an executor session.
- `lead-agent-orchestration`: ephemeral `spawn_agent` helpers are lead-only;
  executor sessions never carry agent-spawning tools.

## Impact

- `crates/harness-core/src/orchestration_config.rs` (executor session marker
  and session arguments), `crates/harness-core/src/launcher.rs` and
  `native_launcher.rs` (launcher-level enforcement),
  `crates/harness-core/src/task_succession.rs` (successor plan),
  `crates/codex-harness/src/executor_cli.rs` (spawn, resume, run, succeed).
- `docs/agent-delegation.md` and `.agents/skills/team-lead/SKILL.md`.
- No configuration schema, receipt or pool layout change.
