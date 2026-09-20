## Why

Delivering a kit change today takes six separately invoked commands with four
absolute path arguments each, fails on a relative `--source` with a message
about legacy metadata, and has no exit from an installation whose recorded
link identity no longer matches an out-of-band edit - the exact state that
blocked delivery and required a manual repair script. Repository skills and
shared defaults are live links while the launcher is an immutable build, so a
checkout can drift ahead of the installed manager with no one-command way to
reconverge.

## What Changes

- Add `codex-harness deploy`: one verb that builds an immutable candidate
  from the source checkout (or reuses `--build`), installs or updates the core
  connection, verifies the installed launcher through its own `--version`
  build identity and executor usage probe, re-reads the installation metadata,
  and prints a single receipt. Component homes default to the current user;
  `--source` must be absolute and a relative path gets its own explicit error.
- Add `deploy --reset`: the explicit repair path for an installation blocked
  by changed link identity. It removes exactly the links recorded as owned
  when they are reparse points (removing a link never touches its target) or
  already missing, preserves and reports every regular file or foreign state,
  writes a reset receipt beside the installation metadata, and then performs
  the normal fresh install. The kit keeps refusing silent repair; reset is a
  deliberate, reported action.
- Add `deploy --all`: after the core connection, run the existing scoped
  component updates (code tools, token workflow, board, subscriptions) in
  order and report each result in the same receipt, so the everyday full
  delivery is one command instead of five.
- Document the one-action flow in the installation guide and command mapping.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `linked-global-kit`: delivery gains a one-action deploy verb with explicit
  blocked-state repair and end-to-end verification of the installed launcher.

## Impact

- `crates/codex-harness/src/main.rs` (dispatch), `install_cli.rs` (deploy
  flow), `crates/harness-core/src/core_install.rs` (owned-link reset with
  receipt).
- `docs/installation.md`, `docs/rust-native.md`.
- Machine installation delivered through the new verb during acceptance; no
  consumer-private data enters shared sources.
