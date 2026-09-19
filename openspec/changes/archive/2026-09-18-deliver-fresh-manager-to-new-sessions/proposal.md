## Why

A Codex CLI session resolves its MCP commands and launcher once, from the
installed registration. That registration currently freezes one manager build
path, chosen from whichever manager happened to run the operation, so
delivering a fresher `codex-harness.exe` needs that exact new binary run by
hand, and any later scoped code-tools update can silently point new sessions
back at an older manager. Windows cannot replace a manager file that running
sessions hold, so delivery must add the fresh build and move the pointer that
new processes resolve, instead of trying to overwrite the old file.

## What Changes

- The registered command for the retained MCP servers becomes the stable
  `<CODEX_HOME>/harness/bin/codex-harness.exe` link instead of a frozen build
  path, so a new Codex CLI session resolves the currently delivered manager.
- Install/Update deliver the freshest integrity-verified published build in the
  owned state; an explicit `--build` still wins, and `--build` becomes optional
  where the running manager already lives in that owned state.
- Re-pointing the managed links is the only mutation; build files, running
  processes and unrelated registrations stay untouched.
- The shared Serena and CodeGraph brokers resolve one private location per
  delivered build generation: a new generation starts its own broker instead of
  retiring a broker that running sessions still use, and a generation whose
  consumers are gone expires on its own idle timeout.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `rust-native-harness`: manager delivery must add a fresh immutable build and
  move the stable link rather than replace a running executable, and new
  consumers must resolve the freshest delivered build.
- `global-code-tools`: the managed MCP registrations must reference the stable
  manager link so a scoped update cannot pin an older manager for new sessions.

## Impact

Install/Update build selection, the generated MCP registrations, isolated
lifecycle fixtures and the installation documentation. No new dependency, no
change to adopted Serena/Nuphus/CodeGraph packages, and no change to running
sessions or their already-resolved binaries.
