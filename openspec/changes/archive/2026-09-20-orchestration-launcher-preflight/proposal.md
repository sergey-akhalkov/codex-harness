## Why

Consuming `team-lead` sessions read skill text through live links into the kit
checkout, while the `codex-harness` launcher is an immutable native build that
changes only through an explicit build and install. After the executor commands
landed in source, an already-installed launcher kept answering
`unsupported command`; a consumer lead first concluded that executors could not
be launched at all and then planned a raw `codex exec` bypass. Both reactions
were wrong, and neither the skill nor the diagnostic explained the version skew
or its remedy.

## What Changes

- The `team-lead` skill gains a launcher preflight before the first dispatch:
  `codex-harness executor --help` must print the executor usage. An
  `unsupported command` answer is classified as a stale harness build with the
  kit update remedy; orchestration stays blocked instead of degrading, and the
  skill forbids repairing the skew by substituting the profile or dispatching
  raw `codex exec`, TUI automation or in-session helpers.
- The delegation guide documents the live-skill versus immutable-build skew,
  its symptom, its cause and the update remedy.
- The manager's unknown-command diagnostic names the attempted command and the
  version-skew remedy instead of a bare `unsupported command; use --help`.
- `--version` reports the adjacent immutable build's source identity when
  present, so an installed launcher can be distinguished from a newer kit
  source; cargo-built binaries keep the plain version line.
- Native tests pin the skill contract and the diagnostic, and the fixed
  launcher is rebuilt, installed through the global lifecycle and verified
  outside this checkout.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `lead-agent-orchestration`: executor dispatch gains an observable launcher
  capability preflight with explicit stale-build handling, and launcher
  diagnostics identify version skew and the update path.

## Impact

- `.agents/skills/team-lead/SKILL.md`, delivered through the existing skill
  links to consuming sessions.
- `docs/agent-delegation.md`.
- `crates/codex-harness/src/main.rs` (unknown-command diagnostic, `--version`
  build identity).
- `crates/harness-core/src/board_cli.rs` (skill-contract test).
- The global native installation: a new immutable build activated through the
  documented lifecycle; no consumer-private data enters shared sources.
