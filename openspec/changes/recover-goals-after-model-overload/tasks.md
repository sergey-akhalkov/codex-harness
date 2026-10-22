# Tasks

Discovery evidence is recorded in [design.md](design.md). The disposable probe established native recovery and the stale-pause counterexample; it did not implement this capability. All implementation and delivery tasks below remain open.

## 1. Preserve the native regression boundary

- [x] 1.1 Promote the discovery's synthetic Responses overload/success sequence into the existing native test ownership beside `task_control_contract.rs`, using explicit installed CLI input, owned homes/processes and retained private evidence. Verify native `serverOverloaded`, blocked goal, status-only automatic resume, same thread/TUI/goal creation identity, preserved budget/accounting and exactly one completed file effect.
- [x] 1.2 Retain the native stale-resume counterexample and console input helper: a real `/goal pause` must be observed before deliberately sending the stale automatic update. Verify the baseline accepts that update, document the race in the native verification guide, and keep it distinct from the future harness regression assertion.

## 2. Implement the ordered recovery owner

- [x] 2.1 Add the small recovery state machine within the existing Rust control ownership: native-error eligibility, goal/turn/epoch binding, five attempts, monotonic exponential waits, bounded jitter and success/manual-resume reset. Verify duplicate events, older turns, partial tool progress, no-goal and unrelated-error exclusions, budget/usage changes and exact exhaustion with deterministic timing tests.
- [ ] 2.2 Serialize supported frontend/user mutations and due recovery dispatch through one owner; keep backend credentials private. Verify pause between the final read and send defeats the stale activation, and interruption, new input, replacement, clear, closure and a newer turn invalidate the candidate. Document the admission/dispatch boundary and native semantics for a user action arriving after dispatch.
- [ ] 2.3 Integrate status-only native activation and checkpoint reconciliation without resending prompts or directly changing databases. Verify successful recovery through installed native components, no duplicate activation after a lost acknowledgement/reconnect, attempt preservation, and no restart from stale state after owner/frontend exit.
- [ ] 2.4 Add compact recovery notifications through the existing frontend path. Verify the same TUI shows the scheduled attempt/delay, distinguishes native blocked state from pending recovery, and reports cancellation, exhaustion or observation failure with a useful next action; update the owning native guide with these states.

## 3. Integrate ordinary and already-hosted sessions

- [x] 3.1 Add the minimal recovery-owned app-server/TUI route to ordinary interactive launch, preserving the inherited terminal; extract only required shared transport/attach behavior from existing owners. Verify `codex` and `codex resume` through the installed launcher without user-supplied `--remote`, without window creation/arrangement, delegation or task-handoff activation.
- [x] 3.2 Reuse an existing hosted session's control owner and preserve unsupported/noninteractive forwarding. Verify one backend/recovery owner per supported session, no silent retargeting of a foreign remote endpoint, and parity for cwd, Unicode, arguments, profiles, explicit overrides, native configuration writes and exit status; document supported and degraded routes.
- [ ] 3.3 Exercise native ongoing-work continuity: overload with pending tool-result delivery and with a long-lived background terminal. Verify same-session result consumption, no duplicated effects/processes, and no blanket requirement that every background terminal end before recovery.
- [ ] 3.4 Exercise failure and cleanup through the ordinary entry point. Verify preparation failure falls back once before payload start, an observer fault after startup preserves existing work instead of launching a second session, and frontend/owner closure cancels retries and reclaims only owned resources.

## 4. Deliver policy through the global lifecycle

- [x] 4.1 Add the versioned enabled/disabled recovery policy to existing installation settings and generated launch registration, retaining explicit opt-out on update. Verify preview performs no writes, incompatible/old registration behavior is explicit, and Check distinguishes configuration/support from exercised recovery; document the lifecycle option in the installation guide.
- [x] 4.2 Deliver and exercise install/update/disable/disconnect/rollback in owned isolated state. Verify saved goals/accounting, credentials, unrelated hooks/settings and already-running sessions are preserved; activation/rollback must not build on ordinary launch or automatically reopen old blocked goals.

## 5. Complete integrated acceptance

- [ ] 5.1 Run the full synthetic native matrix through a fresh installed ordinary session in an independent owned consumer outside this checkout: first use, overload recovery, repeat/resume, five-attempt exhaustion, authoritative cancellation races, changed limits/errors, uncertain acknowledgement, pending work, closure and rollback. Retain exact runtime identities, bounded evidence and verified owned cleanup; no live service overload or paid model call is required.
- [ ] 5.2 Run affected launcher/control/lifecycle native suites, applicable Rust formatting/lint/ownership checks, source hygiene and documentation links. Validate this change strictly, reconcile evidence and remaining limitations with the owning guides, and mark tasks complete only for behavior actually implemented and accepted.
