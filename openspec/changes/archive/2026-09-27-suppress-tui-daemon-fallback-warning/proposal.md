## Why

Starting `codex` with an explicit profile (for example the kit-linked Grok or
the local Z.AI profile) or with the kit's live shared defaults makes codex-cli
0.157 run the TUI against an embedded app-server: profile layers and broad
`-c` overrides are excluded from its shared background server. The CLI still
considers auto-starting that server and renders a startup warning,
`Running without the shared background server: --profile requires embedded
mode.` (or the command-line-override variant for ordinary kit starts). The
session works, so the line is everyday noise on every kit TUI start.

## What Changes

- Classify the TUI launches that always run embedded - those carrying live
  shared defaults or an explicit profile - and pass codex-cli's config-level
  opt-out `-c features.daemon_auto_start=false` before the caller's arguments.
- Keep non-TUI commands (`exec`, `review`, `debug`, management commands),
  `--remote` sessions, explicit `--no-daemon` and daemon-capable starts
  without kit layers exactly as before.
- Leave session semantics unchanged: those launches were already embedded;
  the opt-out only stops the unused daemon auto-start and its warning.
- CLIs without the feature ignore the unknown feature key, so the older
  supported CLI range keeps launching.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `linked-global-kit`: ordinary kit TUI sessions stay embedded without the
  shared-daemon fallback warning; non-TUI, remote and daemon-capable launches
  keep their existing invocation.

## Impact

Native launcher argument policy and ordinary launch composition, their oracle
and integration tests, and the linked-kit requirement. No new dependency, no
Codex CLI change, and no effect on executor control hosts or remote sessions.
