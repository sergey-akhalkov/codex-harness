## Context

`build_identity::check` classifies an installed immutable build as `Healthy`,
`SourceStale`, `SourceUnavailable`, `Altered`, `Missing` or `Incompatible` and
derives admission flags (`management_allowed`, `serving_allowed`,
`runtime_allowed`). Today `SourceStale` and `SourceUnavailable` keep management
and serving admitted but set `runtime_allowed = false`, so source-consuming
commands (executor dispatch, task control, Codebase Memory) refuse to start with
"source-consuming runtime is disabled" whenever the checkout differs from the
delivered build. Binary rolling semantics already exist: delivery publishes a
new immutable build and re-points the stable links without replacing artifacts
that running processes hold. Tests currently assert the stale-source refusal.

## Goals / Non-Goals

**Goals:**

- Decide source-consuming launch admission by recorded binary and metadata
  integrity only; keep refusing missing, altered or incompatible builds.
- Keep stale and unavailable-source reporting in Check, diagnose and deploy
  receipts, including the explicit deploy action, without disabling runtime.
- Keep ordinary launch behavior unchanged for integrity failures and the
  degraded-launcher fallback for genuinely unavailable harness inputs.

**Non-Goals:**

- No whole-session pinning of harness child processes: shared brokers are
  account-wide by design, so per-spawn resolution of the delivered build stays
  the boundary; running processes still keep the artifacts they hold.
- No automatic deploy, watcher or startup compilation.
- No weakening of binary-integrity gates or build metadata validation.

## Decisions

1. **Admission mapping, not per-command bypass.** `SourceStale` and
   `SourceUnavailable` set `runtime_allowed = true` after their recorded
   binaries and metadata verify; their action text becomes an explicit
   build/update reminder stating that launches continue on the delivered build.
   `Altered`, `Missing` and `Incompatible` keep today's refusals. A per-command
   escape flag was rejected: it leaves the breaking default in place and adds a
   second code path to test.
2. **Reporting ownership stays with Check, diagnose and deploy receipts.**
   Admitted commands print no staleness noise; stdout remains machine-readable
   and stderr stays empty on success. `check --build` keeps reporting
   `source-stale`/`source-unavailable` status with `runtime_allowed: true` and
   its existing nonzero health exit; no receipt schema changes.
3. **Launcher fallback classification.** Source staleness alone no longer
   classifies an ordinary launch as degraded; missing, altered or
   metadata-incompatible build inputs and unavailable sources keep the existing
   degraded fallback that still starts the original CLI.
4. **Test contract inversion.** The stale-source gate tests flip from asserting
   refusal to asserting success on the delivered build plus continued Check
   reporting; integrity-failure refusals (altered/missing binaries) remain
   covered unchanged.

## Risks / Trade-offs

- [An older delivered binary consumes newer source-owned configuration with
  schema drift] → Source-owned data stays live by specification and readers
  must tolerate forward-compatible edits; Check and diagnose keep reporting the
  source-ahead relationship until the next deploy, and the source-stale build is
  never reported as healthy.
- [Stale builds can run longer because nothing forces a deploy] → Accepted by
  this change: deploy is the deliberate switch for new processes; receipts keep
  the action visible instead of manufacturing an outage.
- [Machine consumers observe `runtime_allowed` flipping to `true` while stale]
  → Documented in the native guides together with the changed action text;
  tests pin the new meaning.

## Migration Plan

Implement behind the existing workspace checks (`fmt`, `clippy`, locked
workspace tests), update the owning guides, then deliver with the standard
one-action deploy from the kit checkout. Rollback uses the existing
`recover-build`/`activate-build` verbs; no data migration is involved.
