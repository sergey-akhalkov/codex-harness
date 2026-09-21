## Why

`executor spawn` dispatched its Windows Terminal tab with `wt -w 0`. In Windows
Terminal 1.24, `-w 0` resolves to the most recently used window of the current
desktop, which is the user's focused window, not the window of the calling
lead: with two projects in two terminal windows, an executor spawned by the
background project appeared as a new tab in the window the user was typing in,
switched that window to the new tab, and the immediate foreground restore ran
before the terminal's asynchronous activation, so focus stayed stolen. The
terminal exposes no supported address (id or name) for the calling process's
window, and every dispatched commandline unconditionally summons the receiving
window.

## What Changes

- Dispatch finds the lead's own terminal window through the console parenting
  of its pseudo console window, briefly holds that window foreground (the
  activation path falls back from `SetForegroundWindow` through input-queue
  attachment, the legacy switch call, and a momentary Alt press), resolves the
  tab there through the most-recently-used rule, and restores the user's
  previous foreground window once the titled tab is observably open.
- Foreground protection now watches from command launch until the terminal's
  activations go quiet, so the asynchronous summon and the created tab cannot
  re-steal focus after an early restore; the user's selected tab in their own
  window is never changed, and a deliberate user switch is never overridden.
- When the lead window cannot be addressed (another virtual desktop or blocked
  activation), the tab opens in the stable per-checkout window
  `codex-harness-<repository>` (created by the terminal on first use) instead
  of the user's focused window; `--terminal-window` targets an explicitly
  named window.
- The named-window fallback keeps its own deterministic targeting and the same
  foreground restoration, so no path can land an executor tab in an unrelated
  focused window.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `lead-agent-orchestration`: adds the deterministic terminal targeting
  requirement for executor dispatch (lead window first, stable named window
  fallback, explicit window override, foreground and selected-tab preservation
  with user switches left intact).

## Impact

- `crates/harness-core/src/task_view.rs`: lead terminal window discovery,
  virtual-desktop guard, activation escalation, tab-completion observation,
  quiet-period foreground restoration and terminal window enumeration.
- `crates/codex-harness/src/executor_cli.rs`: targeting decision between the
  lead window, the named per-checkout window and `--terminal-window`, plus the
  spawn usage text.
- `docs/agent-delegation.md`: spawn terminal behavior and the platform
  constraint that makes the fallback necessary.
- Ignored interactive tests verify both paths against real Windows Terminal
  windows; unit tests cover name derivation, validation and argument shape.
