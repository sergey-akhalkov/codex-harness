# Reap CLI session process tree

## Why

Interactive Codex sessions launched through the harness wrapper currently opt out of Windows Job containment so that kit-managed background services can outlive a session. Agent shell tools then `Start-Process` hidden helpers that survive after the parent Codex process dies, leaving working `pwsh` trees that consume hundreds of megabytes until someone hunts them by name. The kit already owns kill-on-close Jobs for MCP and indexer workers; the interactive CLI is the gap.

## What Changes

- Put the ordinary upstream Codex CLI process tree in a kill-on-close Windows Job for the lifetime of the harness launcher wrapper.
- Preserve interactive stdio and console inheritance so the TUI still attaches to the calling terminal.
- Reap remaining session descendants when the CLI root exits, the wrapper dies, or the console is closed.
- Keep independently started kit services (xAI shim, shared MCP brokers, CodeGraph workers) outside that session job.
- Do not impose helper memory/CPU caps on the interactive session job.
- Do not add a machine-wide process-name reaper or scheduled scavenger.

## Capabilities

### New Capabilities

### Modified Capabilities

- `rust-native-harness`: Ordinary interactive `codex` launch through the native launcher must own the session process tree and reap it on session end, without wrapping shared kit services or changing argument/stream/exit contracts.

## Impact

`native_launcher::run`, `process::CommandSpec` console inheritance, and Job wait/reap for a foreground CLI with no execution deadline. Shared brokers and the xAI shim keep their existing separate Jobs. Existing `command()` test helper that spawns the registered upstream with redirected pipes stays on `std::process::Command`. No Codex CLI flag changes.
