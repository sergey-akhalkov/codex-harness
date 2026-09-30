## Context

See proposal.md. The supplied symbol query currently succeeds; the original
broker's termination cause is not recoverable from the rotating local log.
Source inspection confirms that transport resets escape Client::invoke, and
all three existing real Serena tests are ignored by the integration command.
During repair the live diagnostic call also returned `pipe reached EOF`.
Worker errors were serialized as untyped strings and a live worker with a
closed channel was not retired. Preserve their transport classification and
retire the unusable worker before the same client-level recovery decision.
The native broker already authenticates endpoints, recreates dead owners and
restores a client's cached route. Reuse those mechanisms.

## Goals / Non-Goals

Recover navigation without restarting the conversation, prove real semantic
behavior at delivery, and retain existing resource and installation ownership.
Do not change limits, remove language support, update packages during startup,
or infer the original crash cause from a TCP error alone.

## Decisions

- Classify transport failures by native error kind, not localized text. Retry
  an explicit safe-read allowlist once, including startup health exchanges,
  against the original deadline. Re-observe the broker instead of killing a
  healthy shared owner. Keep existing cause-specific retirement bounded and
  subject to safe replay. Blind retry of every MCP call can duplicate edits.
- Inject a TCP failure at an owned test endpoint while forwarding to a real
  Serena broker. Assert the request reached the backend, the answer was lost,
  the read recovered and the edit was not repeated. Replace an owned broker
  between requests and verify cached route restoration and independent roots.
  Pure mocked error classification cannot establish this behavior.
- Use native semantic acceptance with adopted dependencies for delivery;
  fail explicitly when connected dependencies are unavailable. Keep the broad
  installed workflow manual, and add a narrower automatic Serena job with
  pinned upstream dependencies and explicitly included ignored tests.
- Keep detailed execution evidence in local storage, synthetic inputs in the
  kit and durable operating guidance in docs/code-tools.md.

## Risks / Trade-offs

- A repeatable crash remains an error after one retry; tests do not establish
  that every possible upstream crash has been eliminated.
- Real language startup adds delivery time. Reuse prepared binaries and run
  a bounded semantic path; do not substitute a successful handshake.
- Test failure cleanup must retire only owned test brokers and preserve
  enough output to distinguish backend, assertion and cleanup failures.

## Migration Plan

Run the regression on the baseline, targeted Rust checks and real acceptance,
then publish through the existing immutable build/deploy lifecycle. Verify the
installed path outside the checkout. Existing MCP processes retain their loaded
binary until their conversation restarts; do not terminate unrelated sessions.
