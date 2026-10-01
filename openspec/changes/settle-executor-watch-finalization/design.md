## Context

See [proposal.md](proposal.md) for the observed user-facing failure.
The native watch loop currently treats the receipt's `completed` state as
terminal before examining the final exit status. The existing receipt-view
path already distinguishes a successful recorded exit from an unsettled
completion. Native observation tests provide real host/receipt fixtures and
cover bounded waiting, interruption and reply holds.

The everyday path is a lead waiting once for a dispatched assignment, reading
its result and then reviewing or releasing the slot. The relevant counterexample
is a host that never finalizes: waiting for a result must still end at the
caller's deadline and preserve the executor's state and original error.

## Goals / Non-Goals

**Goals:** bind a successful watch return to the current finalized native
outcome, retain readable result evidence, and keep the existing timeout and
reply semantics.

**Non-Goals:** change inference, retry model work, add a dispatcher or journal,
wait for the terminal frontend to close, or repair unrelated lifecycle paths.

## Decisions

Use the existing native receipt and completion-classification facilities to
decide whether success is ready. Keep observing the same receipt while the
turn has ended but the host's final outcome is pending. A fixed sleep would
only move the race and add delay to every completed run. Waiting for frontend
exit would break the supported interactive presentation.

Distinguish pending finalization from an established output defect. Once the
native owner has finalized the outcome, a missing or unusable final result
must remain an explicit defect rather than successful completion. Preserve
the existing bounded result reader and original failure details. Historical
coverage and unresolved reply holds keep their existing owners and exit
meanings.

Exercise the transition through the actual `executor watch` command with an
owned native fixture that publishes an intermediate completion and later the
final outcome. This separates the two events deterministically without a
model request or terminal scraping. Run the same assertions in text and JSON
mode. Reuse existing fixture and process ownership instead of another watcher.

## Acceptance

The independent acceptance owner supplies a frozen checker outside the
implementation's writable paths and invokes the built native CLI. It must
reject the unchanged early-success behavior and an implementation that merely
prints a success marker. It verifies that pending finalization cannot return
success, finalized success includes the correct run/result, and an unfinished
finalization reaches the documented timeout without stopping or redispatching
the run. A deliberately wrong or skipped solution must fail this check.

The implementation also passes affected native watch, failure, interruption,
legacy-coverage and reply-hold regressions. Validate formatting and scoped
clippy. Inspect the delivery plan through the existing deployment preview
against owned isolated installation inputs; retain its preparation cost as
part of the real workload. A preview is not proof of applied installation.

For the improvement comparison, both arms receive the same committed task and
frozen checker. Neither receives the other arm's patch, result or Git history.
The planner/checker and experimental supervisor remain unchanged; experiment
outcomes do not themselves mark this implementation accepted or published.

## Risks / Trade-offs

- A lost finalizer could otherwise wait forever: preserve the caller's deadline
  and report incomplete finalization with its detail locator.
- Treating every temporarily missing message as a defect would retain the race
  in a different form: distinguish pending and finalized observations.
- A fixed-delay test can pass without exercising the race under load: use an
  explicit controlled transition and calibrate the independent check against
  the original failing behavior.

## Migration Plan

The fix uses the current command and receipt contract. For an experiment,
build and consume isolated baseline/candidate installations. Accepted delivery
uses the existing global installation lifecycle and its retained previous
build for rollback; no receipt migration or manual session restart is added.
