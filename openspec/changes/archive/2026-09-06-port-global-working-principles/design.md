## Context

See proposal.md for motivation. The source philosophy is 12,844 bytes and contains both reusable principles and contracts specific to opencode-kit. The installed CLI is 0.153.4. The current Codex home is C:/Users/noilw/.codex, with neither AGENTS.md nor AGENTS.override.md present at inspection. No project-specific instruction-file replacement was found in the relevant user-config fields.

The user confirmed adapting disputed review, readiness, and retry rules to the specification and actual risks. Global loading was explicitly requested during the same task.

## Goals / Non-Goals

**Goals:** Keep the full operational philosophy small and self-contained; verify actual native loading across projects; maintain traceability without increasing every session's context.

**Non-Goals:** A general cross-platform installer, custom prompt injection, model changes, new MCP/skills, or guaranteeing behavioral correctness from instruction presence alone.

## Decisions

1. Store the canonical operational text in global/principles-of-work.md. Keep provenance, comparison, and verification in docs/principles-port.md and docs/global-instructions.md. This keeps interpretation history out of the runtime prompt.
2. Prefer a file symlink from the current Codex home AGENTS.md to that source. Native global discovery loads the actual text. A pointer paragraph would require an extra file read; a skill would be on-demand. If symlinks are unavailable, a verified deployed copy is acceptable with explicit refresh instructions and documented snapshot semantics.
3. Recheck both global AGENTS files immediately before activation. Existing files are never overwritten by the initial installation path. Preserve all unrelated configuration and runtime state.
4. Apply user-confirmed rules: review and tests by risk and specification; retry based on failure cause and progress; production evidence appropriate to the claimed outcome. Preserve the original autonomy, integrity, simplicity, reuse, and evidence principles in plain language.
5. Use codex debug prompt-input to verify model-visible instruction items, not a model's self-report. Inspect only the expected principles and probe markers; do not publish the full host prompt.
6. Use temporary Git repositories outside the harness, one with local instructions. Verify every principles section and full normalized source content; check local guidance also loads. Inspect the host link and source hash. Document that clients sharing this Codex home inherit the configuration at session start, subject to ordinary overrides.

## Risks / Trade-offs

- A symlink depends on the checkout location → document relinking after moving it and verify the resolved target.
- A global override or another CODEX_HOME selects another source → document precedence and include both files in diagnostics.
- Instruction changes affect future sessions globally → keep one reviewed source and concise, portable rules; record substantive user decisions.
- Global text consumes prompt budget → consolidate repeated concepts and move the source comparison and installation notes out of the loaded text.
- Prompt presence does not prove future task outcomes → scope verification claims to loading and semantic review; behavioral evaluation belongs to future observed development tasks.

## Migration Plan

Write and inspect the portable text and adaptation record, activate it only after confirming the inspected host paths remain unchanged, render initial prompts in isolated repositories, then record evidence and completion. Rollback removes only the verified activation file/link created by this task; it retains the repository source and unrelated state. Source opencode-kit remains read-only.
