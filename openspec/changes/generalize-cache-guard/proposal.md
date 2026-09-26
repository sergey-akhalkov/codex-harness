## Why

The cache-loss guard is currently hard-coded to DeepSeek even though native per-response usage records are provider-neutral, and its diagnostics are written directly to the host terminal where they can collide with the native Codex TUI input line. Other managed executor profiles therefore lose repeated-cache-loss protection, while even useful diagnostics are presented outside Codex's native warning surface.

## What Changes

- Monitor exact-session per-response input/cached counters for every managed executor profile, without a static provider or model allowlist.
- Treat cache support as proven at runtime by a valid warmed response, not by provider branding or the mere presence of a counter field; unsupported or never-warmed runs do not trigger a stop.
- Preserve the existing conservative warmup and repeated-loss thresholds and the stop/partial-work/recovery guarantees while replacing DeepSeek-specific wording with the actual resolved provider and model.
- Keep routine status transitions in the run receipt and host log; surface genuinely degraded coverage through the native Codex warning mechanism rather than `println!` output that can interfere with the TUI input line.
- Record honest warning-delivery state. A compatibility surface with no native TUI warning consumer uses receipt/log fallback and does not claim that the user saw a warning.
- Preserve the cache-loss stop as a non-success terminal outcome with its existing recovery path; it is not downgraded to a dismissible warning.
- Coordinate the requirement with the active native-harness consolidation change without weakening either change's accepted executor guarantees.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `agent-delegation`: repeated cache-loss protection becomes provider-neutral with runtime-proven support, and cache-guard diagnostics use the native warning surface when one is attached.

## Impact

Primary owners are the executor cache monitor, control/observation hosts, native frontend attachment, receipts, and their Rust tests. A frontend-facing app-server warning adapter may be required because Codex warning notifications are server-to-client; externally maintained Codex workflow files and provider routing remain unchanged. Documentation for executor delegation and native checks must follow the delivered behavior.
