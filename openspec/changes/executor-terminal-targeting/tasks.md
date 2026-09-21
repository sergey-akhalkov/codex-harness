## 1. Targeting and focus mechanics

- [x] 1.1 Discover the lead's terminal window from the console's pseudo console window parenting, and skip it on another virtual desktop (`crates/harness-core/src/task_view.rs`), with a live check against real Windows Terminal windows
- [x] 1.2 Add activation escalation for background processes (direct call, input-queue attachment, legacy switch, momentary Alt press) with verified resulting foreground, verified live between two real terminal windows
- [x] 1.3 Replace the immediate foreground restore with launch-to-quiet watching that undoes every terminal activation, never touches the user's selected tab and leaves deliberate user switches alone; verify live against the repeated activation of a real dispatch
- [x] 1.4 Observe tab creation through the fixed tab title before restoring the lead path's foreground, so the most-recently-used resolution cannot be redirected back into the user's window

## 2. Executor dispatch integration

- [x] 2.1 Target the lead window by default, fall back to the stable per-checkout window `codex-harness-<repository>` when the lead window is unavailable or activation fails, and add `--terminal-window` for exact named targeting
- [x] 2.2 Derive and validate the fallback window name from the source checkout (slug, `;`/control rejection, length cap) with unit tests
- [x] 2.3 Update spawn usage text and `docs/agent-delegation.md` with the behavior and the platform constraint

## 3. Verification

- [x] 3.1 Unit tests: named targeting args without focus flags, native separators, name derivation and validation
- [x] 3.2 Ignored interactive tests: lead-window dispatch (tab in the lead's own window, no new window, foreground restored, tab closes itself) and named-window fallback (window reused by the second tab, foreground restored)
- [ ] 3.3 Run the affected native suites, fmt/clippy and the source/ownership checks; record environment-dependent unrelated failures separately
- [ ] 3.4 Rebuild and update the installed launcher through the installation lifecycle and verify `executor --help` reports `--terminal-window`; leave the user's next real spawn as the final acceptance path
