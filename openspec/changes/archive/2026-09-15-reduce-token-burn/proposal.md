## Why

Measured local evidence (rollout logs, Sep 12-14) shows about 1.95 billion
tokens across 68 sessions, with roughly 80% concentrated in a few marathon
threads that repay their whole history every turn, a default reasoning effort
drifted to "max", a fixed instruction floor of 19-28k input tokens per session
start, and MCP/Apps surface that is catalogued but almost unused (Graphify 7
real calls in 3 days, Apps connectors 5 calls with 125 tools, CodeGraph using
2 of 10 tools, Serena carrying 9 memory/onboarding/config tools the kit already
replaced with native Git records).

## What Changes

- Per-model default reasoning effort in the launcher: "zai/glm-5.3" gets "max",
  "xai/grok-4.6" and the Astra family get "xhigh", applied only when the user
  selected no explicit effort, profile or harness effort selector.
- Retire Graphify from the managed MCP selection the same way Codebase Memory
  was retired: registration removed by update, shared packages and saved graphs
  preserved, rollback remains an explicit local route.
- CodeGraph MCP exposes only "codegraph_search" and "codegraph_detail";
  deliberate "index", "sync" and "status" move to a native CLI control command
  outside model sessions; internal bounded watch/catch-up is unchanged.
- Serena proxy filters memory, onboarding, "initial_instructions" and
  "get_current_config" tools from "tools/list"; native Git memory records stay
  authoritative; an explicit environment escape hatch keeps debugging possible.
- Portable defaults set the native Apps connector feature to "false" while
  machine-local configuration keeps precedence to re-enable it; plugins stay
  opt-in. Removing the locally installed GitHub plugin is a machine-local
  action recorded in verification, not a kit requirement.
- Compress "global/principles-of-work.md" to a bounded instruction floor,
  preserving every normative rule, removing Graphify references and adding
  session-lifecycle economy guidance; update owning docs and memory records.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- "global-code-tools": managed selection shrinks to Serena, CodeGraph and
  Nuphus; CodeGraph exposes a bounded query surface with maintenance operations
  on a native CLI; Serena tool filtering becomes explicit.
- "token-efficient-agent-workflow": per-model default effort, session-lifecycle
  economy guidance and a lean default Apps surface.
- "global-working-principles": the portable principles document gains a size
  bound with all normative content preserved and session economy included.

## Impact

- "crates/harness-core/src": launcher argument policy, portable config
  precedence, CodeGraph catalogue/hints, registration retired projection.
- "crates/codex-harness/src/mcp_cli.rs": CodeGraph control command.
- "tools/code-tools": Serena proxy filtering, registry catalogue.
- "global": harness.config.toml, code-tools.json, tool-resources.json,
  principles-of-work.md.
- "tools/activation.psm1": stop provisioning the retired Graphify runtime.
- Docs and memory records: code-tools, token workflow, project decisions.
- Tests: launcher effort mapping, portable feature precedence, catalogue
  surface, proxy filtering, registration retirement projection.
