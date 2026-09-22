## Context

See proposal.md. Existing owners already provide process capture and local receipts, scoped input hashing for structured inspections, executor slot synchronization, bd bookkeeping and token reports. The current source audit does not enforce the accepted principles limit. Ordinary hooks remain off.

## Goals / Non-Goals

Goals: remove repeatable agent-side mechanics from ordinary reports, dispatch, verification and feedback; preserve existing command consumers and make the new paths globally usable.

Non-goals: infer complete test coverage, semantic task success, permission, similarity or safe cache reuse; create a general policy engine, new tracker, scheduler or model-backed helper.

## Decisions

1. Extend existing binaries and modules. No new executable or dependency is needed. A shared owner-path declaration serves both token-audit and native hygiene checks; the size guard lives in the source-check owner. Test failures identify exact invariant violations. Text compression preserves each normative rule.
2. Keep full JSON stable; make human report/findings presentation bounded, with an explicit complete presentation route. Persist the original report under bounded local evidence and provide scoped detail reads over that report, not a second scan. Baselines remain unchanged. Known errors and coverage are never filtered away.
3. Add an assignment-file input to executor spawn/resume rather than parsing prose. Required fields capture objective, declared inputs, owned outputs, invariants and acceptance. Existing inputs must be regular files under the slot; output validation walks existing ancestors so a new output is accepted but symlink escapes are rejected. The launcher generates checkout/base context after allocation and before opening a model conversation. Free-text dispatch is unchanged.
4. Extend harness-observe with optional scope and repeated input/provenance paths. Reuse its process capture, unique evidence directory and command argument array. Record executable and selected file hashes before/after plus bounded Git HEAD/status; do not collect environment secrets. Distinguish unchanged, changed and unavailable, and preserve the command's true exit. Missing declared inputs fail before the child is started. No result cache or automatic test selection.
5. Expose board operations through codex-harness feedback with structured caller decisions, using board_feedback and board_cli instead of copying algorithms. Configuration supplies thresholds. Read-only ledger/candidate operations and mutation operations are explicit. Preserve partial-operation results and make reruns observe retained history. Integration tests use a synthetic local bd board only.
6. Replace obsolete mechanical recipes in owning docs with these entry points. Protected OpenSpec workflows are untouched. Updates to project-verification document the user-requested native feature rather than autonomous skill evolution; existing restrictions on automatic mutation remain intact.

## Risks / Trade-offs

- Incomplete declared inputs cannot prove full verification freshness: receipts name coverage and leave external state unknown.
- Generated briefs can validate paths but not whether the agent chose all necessary files: acceptance remains agent-owned.
- bd operations are not a transaction: preserve applied actions, expose failure, and test recovery rather than claiming atomicity.
- Smaller output is not measured subscription savings: retain counters and compare output volume/avoided steps only, with end-to-end token benefit explicitly unproven.
- New commands can be bypassed by arbitrary shell actions: enforcement claims cover their entry points and native source checks only.

## Migration Plan

Implement and test in isolated executor slots from the committed planning base. Integrate accepted commits, update owning guidance, run native regression and hygiene checks, deploy through immutable builds, and exercise each new installed route outside the kit checkout. Preserve existing lifecycle rollback and local evidence; no remote publication.
