## Why

The desktop must remain usable while multiple agents build, test and use local tools. An optional command wrapper cannot provide this guarantee: direct launches bypass it, and independent per-command caps can add up across sessions. The owner selected one aggregate 75% CPU budget for all local agent sessions and their tools across projects, with an explicit way to run without that cap.

## What Changes

- **BREAKING**: Make account-wide agent CPU admission automatic on Windows, before useful child execution, instead of depending on instructions to invoke a particular shell or batch wrapper. The normal aggregate ceiling is 75% of host CPU, not 75% per session.
- Cover the installed Codex entry points, interactive and executor/resume routes, their descendants, and kit-owned shared MCP/backend processes. Report incomplete coverage during installation, updates and recovery; do not claim all-session coverage while an active route remains outside it.
- Provide an explicit, visible uncapped invocation that leaves the normal budget and other sessions unchanged. Keep machine policy and process identities outside tracked source.
- Reuse the existing Rust process containment and heavy-command machinery while separating shared CPU accounting from per-session cleanup, memory and batch deadlines. Avoid multiplying nested CPU limits or serializing whole interactive sessions.
- Preserve upstream launch recovery through an independently installed bootstrap. The owner explicitly selected fail-open behavior: if the requested CPU cap cannot be established, launch the requested agent with a visible warning and report degraded coverage. Failure must never silently look like successful enforcement or start a duplicate payload.
- Verify both kernel membership/settings and measured aggregate CPU consumption through installed real entry points, including concurrent sessions, non-shell launches, nested work and shared services.

## Capabilities

### New Capabilities

- `shared-agent-cpu-budget`: Automatic aggregate CPU admission, coverage, explicit exceptions, lifecycle isolation, diagnostics and measured global acceptance for local agent workloads.

### Modified Capabilities

- `linked-global-kit`: Reconcile checkout-independent upstream launch availability with the requested default CPU policy and an explicit uncapped recovery path.

## Impact

Primary implementation owners are `crates/harness-core/src/process.rs`, `native_launcher.rs`, `heavy_command.rs`, `task_runtime.rs`, `process_service.rs` and `broker_launch.rs`, together with the installed CLI, executor and installation owners under `crates/codex-harness`. Existing native launcher, process, heavy-command and service tests provide the initial verification surfaces.

This is Windows development-host resource management, implemented in Rust using the existing Windows API dependency. It does not change provider/model selection, billing, project semantics, remote compute, or unrelated desktop application limits. No new model server, process-name kill loop, scheduled suspension loop or repository-specific limiter is required. Documentation and local policy migration belong to the existing installation and native-command guides. This proposal authorizes no implementation, installation or live process mutation.
