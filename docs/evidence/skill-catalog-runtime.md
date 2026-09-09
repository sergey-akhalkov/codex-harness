# Host skill catalogue across automatic compaction

2026-09-09. The owning [session-awareness requirement](../../openspec/changes/autonomous-skill-evolution/specs/skill-session-awareness/spec.md)
requires current skill awareness before the immediate relevant continuation after
mid-turn automatic compaction. Ordinary context/diagnostic/Stop hooks remain
disabled. Earlier hook-based evidence does not establish an allowed replacement.

The installed original executable reports Codex CLI 0.153.4 and has SHA-256
`444A3F0008050605CAE73CD9B7A2DCAC61294062DFAAB56DD20430FD6498518B`.
The matching upstream release tag resolves to commit
`3d2ee51ca2d5db578f328aa75e20aa22c0197c9a`. Source analysis below is pinned to that
commit. Matching version/tag is source provenance, not a local reproducible-build
claim for the installed binary.

The host catalogue is captured when a new
[TurnContext is constructed](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/core/src/session/turn_context.rs#L966-L1024).
Its [HostSkillsSnapshot](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/ext/skills/src/host_snapshot.rs#L7-L23)
holds an immutable skill-discovery result. World-state reconstruction is real,
but its host catalogue comes from that turn's snapshot.

[Mid-turn compaction](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/core/src/session/turn.rs#L480-L508)
receives the existing step and world state. The loop then captures a step using
the same TurnContext and
[rebuilds world state before sampling](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/core/src/session/turn.rs#L344-L402).
That order does not itself refresh the host snapshot.

The ordinary TUI responds to a skills-change notification by forcing a service
reload and updating its selector. The
[catalogue request handler](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/app-server/src/request_processors/catalog_processor.rs#L472-L540)
does not replace the active turn's snapshot. Also, the
[host provider](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/ext/skills/src/provider/host.rs#L23-L85)
can read current file contents for an already loaded resource while deriving its
catalogue from the old discovery result. Reading a changed existing skill is
therefore not a sufficient counterexample: the distinguishing case adds a new
skill name during the running turn.

A separate Astra senior investigation traced this path and the parent checked
the immediate compaction/continuation ordering and provider contract. The source
supports a limitation of this host-catalogue path; it does not prove that every
possible custom integration is impossible. No ordinary hooks, experimental
executor-discovery workaround, upstream binary or global service were changed.

The [Rust ordinary-TUI probe](../../crates/codex-harness/tests/skill_catalog_refresh.rs)
uses an owned CODEX_HOME/Git workspace and a
[local canned Responses transport](../../crates/codex-harness/tests/fixtures/skill_catalog_responses.rs),
with no model inference or real credentials. It binds a distinct provider to an
ephemeral loopback port. A fake usage count drives the installed CLI's actual
automatic-compaction branch; the summary is canned, while native compaction and
the following request are observed. This does not exercise model skill use.

The first delegated run passed in 24.62 s. Parent review tightened the oracle:
missing compaction/continuation, empty sampling evidence, failed fresh-turn
control, unexpected request order, nonzero exit or leftover owned processes now
fail. The native executable is an explicit `HARNESS_NATIVE_CODEX` input, rather
than a machine-local source constant. A 30-second native tool call gives the
watcher time to act, and the late skill is created while that call is observed
in flight. The test waits for the first turn's actual completion before sending
the positive-control task. Elapsed time and next-turn discovery are not direct
proof that a watcher notification reached the TUI before compaction.

Both strengthened runs passed and observed the same limitation:

| Experimental context | Duration | First continuation after compact | Following user turn |
| --- | --- | --- | --- |
| Explicit false override | 41.08 s | Late skill absent | Late skill present |
| True, as selected in the global profile | 41.30 s | Late skill absent | Late skill present |

The four captured POST requests are first sampling, compaction, immediate
continuation and the next user turn. Each names Astra. Native sampling logs
confirm the corresponding experimental-context value; rollout records include
the real `compacted` event. The early skill appears in the original request;
the late marker is absent from that request, the tool output and canned summary.
Both processes exit naturally with code 0, zero remaining job members, no
transcript truncation, and the configured 512 MiB/50% CPU job limits.

Retained evidence under `%LOCALAPPDATA%/codex-harness-evidence/`:
`skill-catalog-refresh-RrhlAk` (initial), `skill-catalog-refresh-Linz7e` (strict
false) and `skill-catalog-refresh-YJfh8v` (strict true). These contain requests,
rollout, native sampling, process outcomes and selected source receipts. The
strict cases use an isolated profile with unrelated Apps, multi-agent, Goals
and memories disabled; they are not full-profile lifecycle acceptance. Both
default and experimental context use the same traced host-snapshot boundary.

```text
cargo test -p codex-harness --test skill_catalog_refresh --offline --locked --jobs 1 -- --ignored --test-threads=1 --nocapture
cargo clippy -p codex-harness --test skill_catalog_refresh --offline --locked --jobs 1 -- -D warnings
```

Set `HARNESS_NATIVE_CODEX` to the pinned original executable and
`HARNESS_CATALOG_EXPERIMENTAL_CONTEXT` to `false` or `true`. No global
configuration, native binary, filesystem feature or shared proxy was changed.
The required hooks-off delivery/use scenarios remain open. Native MCP change
notifications were also inspected as an alternative: the pinned
[client handler](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/rmcp-client/src/logging_client_handler.rs)
only logs tool/resource/prompt change events; those notifications alone do not
establish a catalogue-delivery route.

Principal Nietzsche completed a read-only inspection of the same pinned commit
3d2ee51ca2d5db578f328aa75e20aa22c0197c9a. There is no established supported
ordinary hooks-off path for the first post-autoCompact continuation in CLI
0.153.4. Additional pinned facts, re-fetched from that commit rather than
current main: [runtime.rs 349-390](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/codex-mcp/src/runtime.rs#L349-L390)
reuses a cached MCP binding when the catalogue revision is unchanged;
[rmcp_client.rs listed_tools](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/codex-mcp/src/rmcp_client.rs#L123-L148)
returns self.tools.clone() after the Apps-cache miss path. This is a bounded
negative for that host-catalogue/MCP listing path, not a claim that every
custom integration is impossible. The existing model-free ordinary CLI probes
(41.08 s false / 41.30 s true) support the limitation and were not rerun.

A user decision remains pending and is not authorized here: a narrow runtime
patch with ordinary hooks OFF, versus an isolated-benefit-approved narrow
compact/resume hook exception. Acceptance tasks stay open.
