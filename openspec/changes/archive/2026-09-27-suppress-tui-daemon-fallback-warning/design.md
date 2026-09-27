# Design

## Codex-cli contract at 0.157.1

The TUI decides the app-server target in `codex-rs/tui/src/daemon_startup.rs`
and `startup_orchestration.rs`:

- `exclusion()` returns `Some("--profile")` for any profile-v2 selection and
  `Some("command-line configuration overrides ...")` for `-c`/`--enable`/
  `--disable` outside a small allowlist. An exclusion forces the embedded
  target; the session never attaches to an already-running daemon.
- `auto_start_daemon` is `config.features.enabled(DaemonAutoStart) && ...`;
  the fallback warning is emitted only as
  `daemon_exclusion.filter(|_| auto_start_daemon)`.
- `features.daemon_auto_start` is inside the daemon-compatible override
  allowlist, so the opt-out itself neither creates an exclusion nor changes
  the embedded target.

Consequences: kit TUI sessions with shared defaults or a profile were already
embedded; disabling only the feature removes the warning and the pointless
daemon auto-start attempt without changing the session. The pinned-tag
upstream tests cover `--profile -> Some("--profile")` and `--no-daemon`
eligibility; the installed CLI was additionally exercised directly
(`features list -c features.daemon_auto_start=false` reports `false`, and the
TUI argument combination parses).

## Alternatives

- `--no-daemon`: codex-cli's own hint and semantically equivalent here, but
  the flag does not exist in the older CLI versions the kit still accepts,
  and an unknown option would abort every such launch.
- Translating profiles into `-c` layers: broad overrides are excluded from
  the daemon exactly like profiles, so this changes the warning text without
  removing it and risks misrepresenting user-authored profile files.
- Keeping the warning: rejected as everyday noise for no operational value,
  since the affected sessions cannot use the shared server anyway.

## Scope boundaries

`exec`, `review`, `debug` and management commands never run this TUI startup
path and several do not accept the option class, so they stay unchanged.
`--remote` sessions keep their endpoint-only invocation, and a start without
kit layers keeps codex-cli's own daemon discovery. A caller's later
`-c features.daemon_auto_start=...` keeps native override precedence.
