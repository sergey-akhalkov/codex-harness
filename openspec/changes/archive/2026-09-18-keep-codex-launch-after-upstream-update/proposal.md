## Why

After a normal Codex CLI self-update the installed launcher compared frozen
executable and package digests and refused to start `codex`. The updated CLI
was already on disk, so the session looked unchanged and the next start died
with an explicit-update error. Ordinary Codex startup must keep working.

## What Changes

- Treat a changed registered upstream executable or `@openai/codex` package
  digest as an upstream update, not as incompatibility and not as a reason to
  refuse launch.
- Start the current file at the registered path with the user's arguments,
  streams and exit status. When that start is otherwise healthy, print nothing
  extra.
- Warn only when harness enhancements actually cannot be applied to the
  current CLI, and still start Codex.
- Do not rewrite launch registration, compile, download or run a second
  session during that start.
- Keep refusing launcher recursion and a missing upstream executable. Check
  may still report stale registration until an explicit harness update.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `rust-native-harness`: ordinary launch must survive an in-place Codex CLI
  update without a harness update first, and must not warn when the current
  CLI is still compatible.
- `linked-global-kit`: harness compatibility checks must not block the
  installed Codex CLI after that CLI is updated.

## Impact

Native launcher preparation, its isolated and integration launch tests, Check
hash reporting (unchanged as a health signal), installation and native-command
docs, and the decision record. No new dependency and no change to Codex's own
updater.
