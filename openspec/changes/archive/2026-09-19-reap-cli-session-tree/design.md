## Context

See proposal.md for motivation. `native_launcher::run` currently calls `command.status()` and documents an intentional skip of kill-on-close Jobs so kit-managed background processes can outlive a session. Helper Jobs already exist in `process.rs` with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and atomic `JOB_LIST` assignment. `CommandSpec.inherit_console` is defined but still redirects unspecified stdio to NUL, so it cannot host the TUI as-is. `Job::wait` requires a monotonic deadline, which is wrong for an interactive session.

## Goals / Non-Goals

**Goals:**
- Contain only the upstream CLI tree spawned by `run`.
- Inherit the calling console/stdio.
- Reap session descendants on root exit or wrapper death.
- Keep `command()` pipe-based tests on `std::process::Command`.

**Non-Goals:**
- Historical scavenger for already-orphaned processes.
- Putting shared brokers or the xAI shim into the session job.
- Memory/CPU caps on interactive sessions.
- Changing task-control / ConPTY dispatch.

## Decisions

- **Session Job in `run`, not `command()`.** `command()` is the test and programmatic spawn helper with redirected pipes. Interactive ownership belongs on the wrapper that waits for the TUI.
- **Fix `inherit_console` instead of `std::process::Command` plus post-assign.** Jobs require `JOB_LIST` at creation. `inherit_console` will skip NUL/`STARTF_USESTDHANDLES`, set `bInheritHandles`, and keep `JOB_LIST`. Redirected stdio combined with `inherit_console` is rejected.
- **Unbounded root wait, then reap.** Add a foreground wait that blocks on the root process handle with no deadline, then terminates leftover job members inside the existing cleanup budget. Dropping the Job remains the crash path via kill-on-close.
- **Do not assign the wrapper itself to the Job.** `contain_current_process` would kill the launcher while it still needs to return the exit code.
- **No silent fallback.** Windows 10+ Jobs are already required. Failure to create or assign the session Job is an error, not an uncontained launch.
- **No process-name reaper.** Existing orphans are a one-time manual cleanup. Recurrence is prevented by containment.

Alternatives: a periodic scavenger is forbidden by ownership rules and would race live sessions. `CREATE_BREAKAWAY_FROM_JOB` is not set, so PowerShell `Start-Process` children stay in the job unless they explicitly break away.

## Risks / Trade-offs

- [A tool uses `CREATE_BREAKAWAY_FROM_JOB`] → Mitigation: still better than today's default; do not hunt by name. Nested Jobs from owned MCP workers remain allowed.
- [Session Job breaks TUI] → Mitigation: inherit_console acceptance plus launcher test that the child is not bound to NUL; keep console Ctrl handler behavior.
- [Shared service accidentally spawned as a CLI child] → Mitigation: xAI shim and brokers keep being started as siblings before `run` waits; do not `AssignProcessToJobObject` on the current wrapper.

## Migration Plan

Ship with the native launcher rebuild/install path. Existing sessions keep previous behavior until restarted. No registration format change. Rollback is the previous launcher binary.
