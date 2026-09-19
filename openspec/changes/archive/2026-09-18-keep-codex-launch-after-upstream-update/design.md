## Context

The installed `codex` command is a harness launcher that execs a registered
upstream path. Installation records that path plus sha256 and, for npm, the
`package.json` digest. Codex's own updater replaces those files in place.
Launch currently treats either digest change as a hard error, so the newer
CLI cannot start until `codex-harness update` rewrites registration.

Existing fail-open behavior already covers unavailable shared source or
config, including a degraded notice when harness enhancements cannot be
applied. Recursion protection and a missing upstream file stay hard failures.
Check already reports an upstream hash mismatch as disconnected harness
health; that remains a registration signal, not a gate or launch warning.

## Goals / Non-Goals

**Goals:**
- After an in-place Codex CLI update, `codex` starts the current registered
  executable.
- Digest mismatch alone is silent when the harness session is still healthy.
- Warn only for actual inability to apply harness enhancements, never as a
  routine update notice, and never as a refused process.

**Non-Goals:**
- Changing Codex's npm/self-update implementation or unlocking a running
  vendor image on Windows.
- Auto-rewriting `native-launch.json` during start.
- Making Check report connected while registration digests are stale.
- Launching an arbitrary `codex` found on PATH when registration is missing
  or recursive.
- A version-gate warning on every newer CLI when no enhancement failure is
  observed.

## Decisions

- **Exec the registered path without a digest warning.** In-place updates
  keep the same path; the new bytes are what the user just installed.
  Re-resolution and registration writes stay on explicit update.
- **Keep npm package env if identity is still `@openai/codex`.** A changed
  manifest digest after `npm install -g @openai/codex` is expected. Skip
  managed env only when the package cannot be read as that identity, without
  treating that skip as a launch blocker.
- **Reuse the existing degraded notice** when shared defaults or other
  harness enhancements cannot be prepared. That is the incompatibility
  warning. Do not add a second notice for digest drift.
- **Check stays strict.** Stale hashes mean the kit registration is behind;
  explicit update refreshes it. Launch is independent of that health.
- **Do not treat a replaced PE as a preflight refusal.** If the file is not
  runnable, the operating system fails the spawn. The harness must not fail
  earlier solely because the digest moved.

## Risks / Trade-offs

- A replaced file at the registered path starts without a harness update.
  That is the accepted Codex update path; recursion and missing files still
  fail.
- Until explicit update, Check stays unhealthy. Preferred over blocking or
  nagging on every start.
