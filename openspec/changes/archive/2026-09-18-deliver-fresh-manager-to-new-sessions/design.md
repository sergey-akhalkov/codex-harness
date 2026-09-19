## Context

See proposal.md. The native lifecycle already publishes immutable, verified
builds under `<state>/builds/<identity>` and never overwrites or removes them.
`harness/bin/*` are file links into the delivered build; `harness/native-launch.json`
is a link to a per-install registration record; the Codex MCP registrations in
`config.toml` currently carry a frozen manager path. Re-pointing a link is safe
while an earlier build still runs, because only the directory entry changes.

## Goals / Non-Goals

**Goals:**
- A new Codex CLI session resolves the freshest delivered manager without
  replacing any file that a running session holds.
- Delivering a newer published build needs no manual "run the new binary"
  step and cannot be silently downgraded by a later scoped update.
- Running sessions, their binaries and unrelated registrations stay untouched.

**Non-Goals:**
- Automatic switching to a freshly built candidate without an explicit
  Install/Update.
- Changing build publication, build reuse or the immutable-build rule.
- Removing old builds or rewriting published `build.json` records.

## Decisions

- Registered MCP command = the stable manager link. The link is validated to
  resolve into an integrity-verified build before it is written, so a
  registration never points at a missing or altered manager.
- Freshest = the newest `builds/<identity>` directory whose full record and
  binary hashes verify. An explicit `--build` still overrides it. Selection is
  deliberately explicit through Install/Update; ordinary launch still never
  compiles, downloads or rewrites registration.
- `--build` becomes optional for core Install/Update when the running manager
  belongs to an owned state; otherwise it stays required with the current
  error. The running manager's own state is used, never ambient discovery.
- Running sessions are preserved by construction: only links change, and each
  build file keeps its identity, hash and location.

## Risks / Trade-offs

- An Install/Update can now adopt a newer build than the operator remembered.
  Accepted: the state only contains explicitly built, integrity-verified
  candidates, and the operation already means "deliver the current manager".
- A stale checkout may still hold a fresher build built from older source.
  Accepted: `--build` selects explicitly, and Check keeps reporting staleness.

## Migration Plan

Install/Update from the current source re-registers the stable link and
delivers the freshest verified build. Existing sessions keep their previous
manager and catalogue until restart; a new session picks up the delivered
manager. Rollback is the previous frozen-path registration.
