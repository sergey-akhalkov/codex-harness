## Context

Windows Terminal 1.24 (verified against its source and installed help) has no
programmatic address for the window of the process invoking `wt.exe`:

- `-w 0` and `-w last` resolve to the most recently used window on the current
  desktop (`WindowEmperor::_dispatchCommandlineCurrentDesktop`), i.e. the
  user's focused window.
- Numeric window ids are internal; they appear only in the identify overlay and
  the notification menu, not through any API.
- `-w <name>` targets named windows precisely, but an already-open unnamed
  window cannot be renamed from the command line.
- Every dispatched commandline activates the receiving window
  (`AppHost::DispatchCommandline` always summons), and the activation lands
  asynchronously around the launcher's exit, including a second activation when
  the tab is created.

## Decision

The lead's window is identifiable: the terminal parents the pseudo console
window reported by `GetConsoleWindow` to its own top-level window. Dispatch
therefore activates that window, resolves the tab through `-w 0` (the freshly
activated window is the most recently used window of the current desktop), and
restores the user's foreground window only after the fixed tab title appears in
the lead window - restoring earlier would make the user's window the most
recently used window again and redirect the tab into it.

Activating a window from a background process needs escalation on current
Windows: direct `SetForegroundWindow` reports success without activating, and
`AttachThreadInput` alone is insufficient, so dispatch falls back to the legacy
`SwitchToThisWindow` and a momentary Alt press. All paths verify the resulting
foreground window; failure selects the named fallback.

The fallback targets a stable per-checkout window name
`codex-harness-<repository>`, which the terminal creates on first use. This can
never resolve to the user's focused window of another project. An explicit
`--terminal-window` keeps exact opt-in targeting for named windows.

## Risks

- The lead window must stay foreground until the terminal resolves the tab
  (observed through its title); on a slow terminal this can hold it for up to
  2.5 s, after which the user's window is restored anyway.
- A window on another virtual desktop is never activated; those dispatches use
  the named fallback to avoid switching the user's desktop.
- If the user deliberately switches windows during a dispatch, the watcher
  stops restoring and leaves the user's choice in place.
