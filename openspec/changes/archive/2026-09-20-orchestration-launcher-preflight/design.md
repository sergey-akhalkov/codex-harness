## Context

See proposal.md for the motivation. The kit currently delivers skills as live
links into the source checkout, while `codex-harness` binaries are immutable
builds selected by the installation lifecycle. The failure was observed with an
installation whose binary predates the executor commands while its linked skill
already required them; both old and new binaries report version `0.1.0`.

## Goals / Non-Goals

**Goals:**

- Make launcher/source skew detectable and actionable at the point of use: the
  lead session, before the first executor dispatch.
- Keep orchestration honest when the launcher is stale: explicit blocker and
  remedy, no silent degradation or improvised bypass.
- Make the manager's unknown-command diagnostic name the command and the
  version-skew remedy.
- Make an installed immutable build identifiable from its own `--version`.

**Non-Goals:**

- No automatic rebuild, auto-update or background version check: builds stay
  explicit and immutable.
- No change to executor dispatch semantics, profiles, worktrees or the board.
- No new versioning scheme for the kit; source identity from the existing
  `build.json` is sufficient to distinguish builds.

## Decisions

- **Preflight by capability probe, not version comparison.**
  `codex-harness executor --help` returning the usage is the check. A stale
  build fails it before dispatch; a capable build proves the exact command the
  skill needs. Version strings cannot distinguish the builds today, and a
  version range would need a second source of truth.
- **Stale launcher blocks orchestration, mirroring `board unavailable`.** The
  lead reports the blocker with the remedy (rebuild/activate through the kit
  lifecycle; the registered source root lives in the installation record) and
  continues only work whose acceptance does not depend on executors. Raw
  `codex exec`, TUI automation and in-session helpers remain forbidden because
  they drop the visible-conversation, steering, board and recovery contract.
- **One generic actionable unknown-command message.** The manager cannot know
  which future commands skills will reference, so the message names the
  attempted command and states the skew remedy once, for every unknown
  command. A per-command allowlist would duplicate knowledge and rot.
- **`--version` reports adjacent `build.json` identity, best effort.** Immutable
  builds already carry a `BuildRecord` with the full source hash; printing its
  prefix when the record sits next to the executable distinguishes installed
  builds without changing cargo-built binaries or adding state. Read failures
  keep the plain version line.
- **Enforcement lives in the skill, not in `check`.** `check` runs from the
  installed binary, which cannot know commands that only a newer source
  introduces; the skill is the artifact that references the command and the
  only place that can compare need against the installed launcher.

## Risks / Trade-offs

- [Wording drift between skill, docs and tests] → The board skill-contract test
  pins the preflight command and the stale/no-bypass wording.
- [Generic remedy text appears for genuinely mistyped commands] → The message
  still starts with `unsupported command '<name>'` and `--help`; the extra
  sentence only adds the skew explanation when instructions referenced the
  command.
- [`executor` is Windows-gated, so the message could mention skew on a
  non-Windows build] → The kit targets Windows x64 only; the preflight and
  orchestration are Windows features, and the diagnostic remains true for every
  kit-documented command.

## Migration Plan

Implement the skill, docs and manager changes; run the native checks; build a
new immutable candidate from this checkout; update the global installation
through the documented lifecycle; verify the installed launcher outside this
checkout (`executor --help`, unknown-command diagnostic, `--version` build
identity) and the consumer-visible skill link. Rollback is the existing
`recover`/`recover-build` lifecycle; skill links follow the checkout, so a
source rollback restores the previous skill text immediately.
