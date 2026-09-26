## Context

See [proposal.md](proposal.md) for motivation. The existing detector already reads exact-session `token_usage_record` events incrementally and owns process termination, but `Monitor::new` filters it to DeepSeek and emits diagnostics with terminal `println!` calls. Control-backed executors attach the native TUI as a second WebSocket client to the harness-owned app-server. Codex 0.157 exposes warnings as server-to-client app-server notifications; it has no client request for injecting a warning into an attached frontend.

## Goals / Non-Goals

**Goals:**

- Reuse the existing exact-session reader, thresholds, detector state, stop path, receipt fields, and recovery command.
- Make provider/model support runtime-proven and avoid maintaining a provider allowlist.
- Preserve native terminal ownership: no cache-guard diagnostic writes to the inherited terminal when a native frontend owns it.
- Prove warning presentation through the real app-server/TUI contract rather than ANSI insertion or a model request.

**Non-Goals:**

- No provider routing, model, reasoning-effort, worktree, slot, or recovery-policy change.
- No claim of a currency cap, diagnosis of upstream cache eviction, or zero in-flight spending.
- No modification of externally maintained Codex sources or OpenSpec workflow files.
- No replacement of the terminal cache-loss outcome with a warning-only notification.

## Decisions

1. **Enable the monitor for every managed executor receipt.**

   Read the resolved provider/model from the existing receipt and instantiate the same monitor for all profiles. Keep numeric validation and duplicate ordering in the detector. A response meeting the existing warmup thresholds proves that this run has usable cached-input telemetry; numeric-but-never-warmed counters remain valid observations but never arm the stop. This avoids treating a provider name or an always-zero field as evidence of support.

   *Alternative considered:* maintain an OpenAI/Z.AI/xAI/DeepSeek allowlist. Rejected because support varies by model and runtime response, and an allowlist would silently lose future profiles.

2. **Keep the loss policy conservative and provider-neutral.**

   Preserve 100,000 input tokens and 90 percent cached input for warmup, 100,000 uncached tokens and 50 percent uncached input for each miss, three consecutive distinct misses, historical warmup on exact resume, and immediate owned-tree termination. Replace hard-coded DeepSeek evidence text with the receipt's actual resolved provider and model. The existing fresh-session recovery remains unchanged.

   *Alternative considered:* provider-specific thresholds. Rejected until measured losses on another route demonstrate that the common threshold is insufficient; no such evidence is part of this change.

3. **Classify diagnostics instead of printing every transition.**

   Startup, first usable usage, warming, armed, and continued-below-threshold states remain receipt/log state. Invalid counters, unavailable evidence, rollout truncation, and session-identity changes are degraded diagnostics. Cache loss remains a terminal non-success outcome with the existing stop/recovery surface.

   *Alternative considered:* route every status line through the warning bucket. Rejected because successful monitoring is not a warning and would train users to dismiss the indicator.

4. **Use a native app-server `warning` notification for an attached TUI.**

   The current protocol direction is server-to-client. Add the smallest harness-owned frontend-facing WebSocket relay for the native frontend connection: authenticate the same loopback capability token, pass app-server bytes unchanged in both directions, and only append a correctly shaped native `warning` notification for the exact thread when the host queues a degraded diagnostic. Wait until the frontend has initialized/resumed the thread before sending. Do not rewrite model traffic, thread history, rollout records, or credentials.

   Record bounded, deduplicated diagnostic state (for example native-sent/undelivered/no-native-consumer) in the run receipt. WebSocket dispatch proves a native-format send, not that the human opened the viewer; wording and checks must not claim user observation. If a future installed app-server exposes a supported client-to-server warning API, prefer that API and retire relay-specific insertion.

   *Alternatives considered:*

   - Direct stdout/stderr: rejected because it competes with the native TUI and caused the input-line defect.
   - Inject a thread item or model-visible history record: rejected because diagnostics must not alter model context.
   - Launch-time config warnings: rejected because they are static, not exact-session runtime coverage, and can contaminate unrelated sessions.

5. **Keep compatibility surfaces honest.**

   The existing fixture renderer has no native warning consumer. It retains receipt/log diagnostics and records that native warning presentation is unavailable. A relay send failure also does not terminate otherwise healthy model work; it records undelivered diagnostic state while preserving the underlying coverage state and terminal outcome.

## Risks / Trade-offs

- [Experimental app-server notification shape changes] → Detect the installed supported protocol before use, keep relay forwarding byte-preserving, and fail to the honest no-native-warning state rather than guessing a payload.
- [A relay defect breaks frontend attachment] → Cover handshake, authentication, bidirectional forwarding, warning insertion, close, and concurrent control traffic with local owned WebSocket fixtures, then exercise installed native TUI attachment before delivery.
- [Warnings can become noise] → Emit only degraded, deduplicated diagnostics; keep routine state out of the native warning bucket.
- [Send success is not user observation] → Record native send/delivery state separately from user visibility and assert receipt wording.
- [Overlap with `simplify-native-harness`] → Integrate against current executor owners, preserve its cache-loss acceptance scenarios, and do not mark either change's shared dependency complete without the combined native path.

## Migration Plan

Implement behind the existing executor entrypoints and immutable build lifecycle. Add deterministic monitor and relay tests first, then native attachment acceptance against an owned temporary installation. Deploy a new immutable build, exercise one non-DeepSeek synthetic provider and one native warning path outside this checkout, and retain the previous build as rollback. Update delegation/native-check documentation only after the installed behavior is observed.
