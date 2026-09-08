## Context

See [proposal](proposal.md). The installed Windows CLI is 0.153.4, reached through the linked PowerShell launcher. The kit currently disables hooks in both the source profile and native base config; `global/hooks.json` is empty and the old diagnostic handler is inert. The source tree contains concurrent implementation of other changes; preserve it and reuse their tests.

RTK v0.48.0 has a native Windows binary and `pipe --filter`, which applies a filter to existing output. Its ordinary Codex initialization installs instructions, while its Claude hook rewrites commands with broader semantics than this kit can safely promise. Current Serena clients reject source/runtime changes and need fresh connections; CBM has explicit indexing with partial PowerShell coverage. Code Mode exists in the parent tool surface, but installed CLI feature activation still needs independent proof.

## Goals / Non-Goals

**Goals:** Preserve execution semantics and raw evidence, keep per-command overhead small, use supported native Codex configuration, and integrate through the existing source-linked lifecycle.

**Non-Goals:** A new agent orchestrator, LLM output summarizers, universal shell parsing, reinstating diagnostics, API billing, Fast mode, rewriting unrelated MCP servers, or claiming a quota percentage from byte counts.

## Decisions

### 1. Filter output from one original execution

Use a small native adapter and pinned RTK v0.48.0 with checksum verification. Codex 0.153.4 canonical hook input contains `command` but omits the actual shell and tty; automatically injecting PowerShell into an ordinary command would break explicit Bash/Cmd calls. Therefore the verified command boundary is `harness-rtk.exe exec EXECUTABLE ARGS...`: it explicitly selects native execution, while the caller's shell still parses its arguments. The hook changes only `exec` to `compact`, emits native `allow` plus `updatedInput.command`, and preserves the tail verbatim. Native argv selects a conservative filter; unsupported formats execute raw. Ordinary commands, shell control constructs and already compact calls are unchanged. Terminal stdout passes raw. This avoids shell guessing and preserves arguments, cwd/environment, stdin/stderr and the child status, including the outer shell's normal exit mapping. It does not select shell aliases/functions; those retain the ordinary shell path. Start with useful git/status/log, search and test forms; extend only with semantic evidence.

Retain original stdout before filtering; stderr stays unchanged on its original stream. Preserve the original exit status. Keep raw evidence in bounded machine-local storage with explicit paths and retention policy; oversize output passes raw rather than being silently shortened. If filtering fails or expands output, return raw output. Do not rerun a failed command. Match only shell calls; nested Code Mode calls rely on the native hook contract. Hook timeout is explicit and short; missing runtime must not prevent raw execution. A native compiled adapter avoids PowerShell startup for every hook.

Alternatives: direct `rtk hook claude`/third-party deny-and-retry hooks, assumed PowerShell wrappers and polyglot wrappers. Rejected for runner/shell changes, missing per-call metadata and avoidable model turns. A sessionshell or post-call transcript cannot prove the omitted per-call fields. The explicit native boundary costs a short call prefix and is the safety condition for automatic compression; no new shell parser or orchestrator is introduced.

### 2. Native activation and lifecycle

Preserve the existing installer transaction, conflict checks and source links. RTK is an external package, while adapter source/build recipe and hook definition are owned here; compiled host artifacts are identified by source/dependency hashes. Enable native hooks only for the installed accepted selection; a fresh core-only install stays hooks-off. Base and profile agree for selected consumers. Preserve explicit suspension, disconnection and rejected diagnostic inactivity. Obtain native trust for the exact managed definition; do not install a blanket trust bypass. Old sessions require restart to discover changes.

### 3. Code Mode and retrieval policy

Enable the supported native Code Mode feature and verify actual tool delivery in a fresh installed consumer. Add concise portable policy plus focused usage reference: batch independent calls, inspect every settled result, print decision-relevant data, keep failures and detail locators, and sequence dependencies. Prefer CBM for graph relationships with verified freshness/coverage and Serena for symbols/references/semantic edits. Reuse their existing resource bounds and caches. Improve redundant interface text or responses only with a measured concrete case; do not remove upstream operations or hide incomplete coverage.

### 4. Scoped edits and evidence reuse

Make `apply_patch` the normal choice for suitable narrow text changes, keeping semantic edits and deterministic scripts for their actual strengths. Extend the existing workflow guidance with explicit invalidation: relevant source/config/runtime changes invalidate affected evidence, unchanged results can be reused. No persistent duplicate cache or universal verification wrapper is needed. Verify with a small retrieval/edit/check scenario and Code Mode batch containing a failure, not tests that merely search prose for keywords.

### 5. Reasoning is a native task setting

Use actual native effort configuration for task boundaries: routine bounded work can use a lower supported effort; ambiguous/high-risk work retains high/xhigh and demanding consultation the existing max level. Preserve the conservative parent default and fixed preferred Grok/backup role assignments. Provide a compact task-selection entry point or existing native override recipe that preserves explicit overrides and forwards arguments unchanged. Verify effective effort through native config/turn evidence. Do not pretend a brevity instruction changes active-turn effort or infer CLI support from the Astra API `configuration_update` item. Native in-turn switching is used only if the installed contract proves it; otherwise boundaries and limitation are documented.

## Risks / Trade-offs

- Filters omit information → retain raw evidence, label compression, bypass exact/machine queries and verify an independent oracle, including failure details.
- Shell/argv differences → conservative accepted syntax and representative quoting, Unicode, cwd, environment and exit-code checks; unsupported forms are untouched.
- Hook cold start or extra runner cost → measure hook latency separately and total raw/optimized command latency. The initial investigation threshold was more than 10% AND 100 ms. After measurements the user explicitly accepted the adapter's 30–60 ms and Codex hook dispatch of 250–313 ms; this observed native overhead is acceptable. Record cold calls separately and do not generalize that acceptance to substantial further delays or weaker correctness.
- Lower reasoning harms completion → retain conservative default and escalation; low effort is for bounded routine tasks with deterministic checks, with a matched native task acceptance example.
- Stale semantic evidence → fresh MCP clients, verified repository roots/index and direct-source coverage checks.
- Lifecycle resurrects diagnostics or overwrites user work → tests for install/update/repair/relocation/disconnect and exact source ownership; preserve unrelated config and dirty edits.

## Migration Plan

1. Build and check the adapter and pinned dependency in owned temporary roots before selection changes.
2. Implement lifecycle and native feature/effort settings, update portable usage and owning decisions.
3. Run focused semantic/error/lifecycle checks, then activate globally and establish native trust.
4. Exercise fresh consumers outside the checkout, record source/build identity, raw/compressed sizes and elapsed times. Record estimates separately from tokenizer/session usage.
5. Close tasks only on current evidence, sync the delta specs and archive. To roll back, disconnect the accepted optimization selection and restore the ordinary hooks-off state without reviving old diagnostics.

## Sources

- [RTK v0.48.0](https://github.com/rtk-ai/rtk/releases/tag/v0.48.0), [pipe filter implementation](https://github.com/rtk-ai/rtk/blob/v0.48.0/src/cmds/system/pipe_cmd.rs), [raw retention](https://github.com/rtk-ai/rtk/blob/v0.48.0/src/core/tee.rs).
- [Native hooks](https://learn.chatgpt.com/docs/hooks): PreToolUse rewrites, trust, nested calls and timeouts; verify against installed consumer.
- [Installed canonical execution input](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/core/src/tools/handlers/unified_exec/exec_command.rs), [hook dispatcher](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/hooks/src/engine/command_runner.rs): command-only rewrite and Windows default Cmd launch.
- [Native configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference): Code Mode and reasoning settings; API settings are not automatically CLI features.
