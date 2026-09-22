## Context

The live executor profile (`ds`) binds `deepseek-flash`, whose model catalog
entry declares `multi_agent_version = "v2"`. In Codex CLI 0.155.1 the session's
effective multi-agent version is resolved as: the `features.multi_agent_v2`
override, else `agents.enabled = false` => `Disabled`, else the model catalog
value, else the `features.multi_agent` (collab) feature. Disabling only the
`multi_agent` feature therefore does not remove the tools for this profile:
the catalog value still selects v2, and v2 children of a v2 model may spawn
further agents. `agent_max_depth` is a v1-only guard and does not bound v2.

## Decision

- Force the effective version to `Disabled` with the built-in configuration
  key `agents.enabled = false` (`-c agents.enabled = false`). This is the
  documented configuration switch and wins over the model catalog.
- Apply it in three places: the executor session arguments (spawn, tui, resume
  and succession), the installed launcher for any process started in an
  executor-marked environment, and a refusal guard on the kit's dispatch
  commands. The arguments make the effect visible in receipts and keep the
  behavior even when a nested launch bypasses the marker; the launcher extends
  it to raw nested `codex` invocations; the guard stops the kit's own dispatch
  path with an actionable error.
- Keep the environment marker (`HARNESS_EXECUTOR_SESSION`) internal to the
  dispatch processes; the lead session never carries it.

## Risks / Trade-offs

- A deliberate bypass remains possible: an executor can clear its environment
  marker and call the registered upstream Codex executable directly, or run the
  kit dispatch with a forged environment. The launcher and dispatch guards
  cover the supported entry points; the acceptance evidence states this limit
  instead of claiming a sandbox.
- If a future CLI renames the configuration key, the launch arguments become
  inert. The offline probe and the launch argument oracle pin the verified
  behavior for the installed CLI.
