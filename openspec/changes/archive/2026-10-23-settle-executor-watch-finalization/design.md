## Context

See [proposal.md](proposal.md) for the observed user-facing failure.
Current source already distinguishes a successful recorded exit from an
unsettled completion in the watch loop and receipt view. The implementation
introduced in `e2395f5` and its native observation regression are reuse inputs.
Reconcile their actual coverage before adding code; zero checked tasks is not
evidence that the fix is absent. Native fixtures cover the host/receipt boundary.

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

Freeze exact source and executable identities before running the comparison.
Use the parent of `e2395f5` as the historical defect control only after verifying
its applicability; use the current candidate for the corrected arm. Keep the
same external checker and controlled event sequence in both. If the historical
input is unavailable or already passes, report the control as unqualified and
investigate; never label current HEAD as failing by assumption. Existing valid
evidence may be reused with its identities and limits.

For each text/JSON mode, hold finalization pending, then publish result and exit;
also exercise never-finalized, failed and empty-result outcomes. Require at
least one premature success from the historical defect control, zero from the
candidate, the exact retained result after successful finalization, and the
specified timeout/failure outcomes with no stopped or replayed run. These are
deterministic correctness outcomes, not a statistical speed claim. An unchanged
current implementation that already satisfies them is retained; a new patch
without additional demonstrated benefit is unnecessary. A failing candidate is
excluded from accepted source/runtime and its evidence remains retained.

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
