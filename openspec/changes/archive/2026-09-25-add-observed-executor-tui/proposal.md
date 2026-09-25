## Why

Executor tabs currently render a plain stream of messages and tool activity that is difficult to follow. The user wants the native Codex TUI while retaining the existing blocking `executor watch` workflow and automatic tab closure when the run ends; the current control-backed execution path already supplies lifecycle events independently of the frontend process.

## What Changes

- Make ordinary pooled executor spawn, resume and restart show the native TUI attached to the exact managed conversation, with the configured model and effort and the existing slot binding.
- Keep `executor watch` as the blocking, receipt-based interface with its existing arguments, exit meanings, bounded result and failure reporting. A finished turn and a closed frontend remain distinct observations.
- Persist the final result and run outcome after accepted work is settled, then close the owned frontend and release only that run's backend resources so the executor tab closes automatically without manual input. An unresolved reply request is not a terminal run.
- Preserve addressed `message`, urgent `stop`, exact-session continuation, partial work, cache-loss protection and process ownership. Losing the only TUI must not leave an executor working invisibly.
- Reuse the native remote-TUI and two-client contract checks, then verify the actual installed executor commands outside the kit checkout. Do not replace the native TUI with another custom renderer or infer completion from the last rollout line.
- Compose with the separate `close-failed-executor-tab` change, which owns terminal-host exit policy and oversized final-message recovery; retain that behavior when adding a TUI.
- Retire replaced custom rendering in the same delivery, preserving explicit presentation spellings through qualified native presentation or the smallest proven compatibility adapter. Follow `simplify-native-harness` for cross-cutting consolidation and instruction reduction; do not retain a second controller for an old presentation.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `lead-agent-orchestration`: native TUI presentation for managed executor runs, compatible blocking observation, automatic owned-surface closure, and preserved control/recovery behavior.

## Impact

The executor CLI, control driver, observation and owned-process integration in `crates/codex-harness`, the reusable task-control transport/frontend facilities, existing Rust executor/native contract tests, the global installation lifecycle, and the delegation/native-command guides. No new provider, model binding, service platform, external dependency or model polling loop is proposed. Existing commands and package identifiers remain compatible. This change owns the base observable-lifecycle requirement; executor-to-lead messaging adds reply holds and actionable waiting observation through separate requirements rather than replacing that block.
