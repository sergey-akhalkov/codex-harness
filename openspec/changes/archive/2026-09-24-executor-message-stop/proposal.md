# Why

A lead can dispatch pooled executors but cannot correct a running one: the default exec backend (`codex exec`) has no inbound-context capability, so the only "steering" remedy is kill-and-redispatch, which wastes partial work. Urgent stops are equally manual: the lead must find and kill processes itself, with no identity check, owned-tree guarantee, tab closure, or honest lifecycle record. Board feedback `codex-harness-pvr.8` records exactly this failure (mid-work correction required kill+respawn twice).

# What Changes

- Add `codex-harness executor message`: address one live pooled executor by checkout/slot/owner (and exact recorded session identity), and deliver a short literal text or a UTF-8 multiline file into the **same** conversation through a real backend-accepted turn, without stop/resume, a new conversation, model/provider/effort changes, or re-sending the task. Distinguish queued, confirmed delivery and error; never present a local file write as model delivery; make the message visible in that conversation's terminal surface; retry safely without silent duplicate delivery; give an explicit result and supported next action for completed/stopped/unavailable runs.
- Add `codex-harness executor stop`: urgently stop one exact executor - verified identity, native interrupt where the backend provides it, bounded termination of the remaining owned process tree, closure of exactly that run's terminal tab, honest receipt update (`stopped`, `already-completed`, partial failure with the surviving resource and next action, unknown exit code stays unknown), preserved files/checkout/partial work, no automatic release or completion claim, safe repeated stop, and a result that survives tab closure. Stop must work during generation and during a running child command.
- Change the pooled exec-mode owning route so executor conversations are backed by a harness-owned native app-server session (loopback, token-protected) driven by the existing tab host, because the verified native inbound route (`turn/start`, `turn/interrupt`, second client) requires an app-server-owned thread. The tab host keeps its current rendering, receipt lifecycle, result recording, Windows Job ownership, lease and slot guarantees. Raw `executor run LAUNCHER` and `tui` mode are unchanged; `message` is honest about unsupported surfaces.
- Update the existing instruction owners (CLI help/USAGE, executor lifecycle documentation, `team-lead` skill, delegation guide) with both commands and the message-vs-stop selection rules, and deliver them through the existing installation lifecycle.

# Capabilities

## Modified Capabilities

- `agent-delegation`: executor command surface, lifecycle/receipt states, control-backed exec sessions, message delivery and stop semantics.
- `lead-agent-orchestration`: concrete lead steering/stop commands and their selection rules (no status-only nudges, stop only for explicit cancellation or confirmed necessity, preserve partial work).

# Impact

- `crates/codex-harness` executor CLI (`executor_cli.rs`), executor observation/lifecycle (`executor_observation.rs`), executor shell preparation, and their native tests.
- `crates/harness-core` task-control connection reuse; no new parallel executor-management or reporting system.
- Kit instructions and skills (`team-lead`), `docs/agent-delegation.md`, `docs/rust-native.md`, installed CLI help.
- Installation lifecycle delivery and installed-launcher verification; no change to pool sizing, profile selection, billing or provider routing.
