## Context

See [proposal.md](proposal.md) for the incident. Installed identities: CBM 0.10.8, Serena 1.7.0, Graphify 0.9.55, Nuphus 0.2.2 and Codex CLI 0.153.4 on Windows with 8 logical CPUs and about 16 GiB RAM. CBM already shares one coordination daemon; its internal memory budget did not contain a 5,844 MiB worker. Saved hashes include 10,888 generated HTML logs. Existing MCP startup retains PowerShell and multiple Python launcher layers. The LSP service currently keys backends by session/agent as well as project and settings, and uses SolidLSP subprocesses without Windows parent-death containment.

The existing source tree has unrelated uncommitted changes. Installation adopts shared packages used by OpenCode. Reusable changes belong to this kit; upstream installations are not patched in place. Global consumer changes and the specifically approved pmac-emulator ignore file are authorized notwithstanding OpenSpec's repository-local planning-home metadata.

## Goals / Non-Goals

Selection update, 2026-09-08: `reduce-subscription-waste` owns the accepted capability set. All native hooks remain off, the separate harness diagnostic carrier is retired, and explicit Serena operations are evaluated separately. The diagnostic broker design below records the implemented historical alternative; its registration, journals, pre-edit work and Stop delivery are not requirements to reactivate. Ownership and isolation still apply to retained services. Installation, recovery and archive must preserve the newer disabled selection.

**Goals:** enforce the indexing envelope independently of native allocation behavior; share only compatible analysis state; retain existing public tool operations and independent diagnostic delivery; reclaim inactive and orphaned work; provide measurable global acceptance.

**Non-Goals:** a common Codex app-server; changes to product sources, controller operation or subscription routing; blindly sharing a mutable active project or browser; replacing existing MCP packages with unverified versions.

## Decisions

1. **Supervise explicit CBM indexing.** A kit MCP adapter retains the native tool catalogue and query capabilities, but routes indexing through a bounded native CLI worker. An account-wide lock admits one job, with two extraction workers, a 1 GiB internal target, a 2 GiB Windows job memory limit, 25% CPU rate and a finite deadline. The native committed index remains the publication boundary. Configuration migration disables auto_index/auto_watch and records original values; native watcher subscriptions already running require an explicit coordinated restart. Later agents receive the explicit-refresh policy. An environment-only memory setting was rejected because the incident disproved its sufficiency. No retry without changed inputs or an explicit new request.

2. **Exclude generated input deliberately.** Add the known HTML-log directory and inventoried archived measurement JSON to pmac-emulator's .cbmignore; keep actual specifications, source files and useful reference documents. Limit native retained source to 8 MiB total and 1 MiB per file; native cross-file passes reread uncached source. Two extraction workers retain the parallel pipeline; one worker selected a different sequential path and failed bounded acceptance. Save original consumer configuration outside tracked sources. Do not delete caches or source archives to appear to fix RAM usage.

3. **Use OS process ownership.** A reusable Windows job helper supports kill-on-close, optional memory/CPU limits, current dedicated-service containment and atomic child admission. Handles are non-inheritable. Containment must precede user code/descendant creation, including protection from a supervisor dying during startup. Other platforms report which enforcement is available instead of claiming Windows guarantees. Only identified historical PSES trees are eligible for the one-time cleanup.

   **Independent service bootstrap:** a cold-start SDK probe showed that detached child processes still inherit the starter client's Windows Job, so closing that client killed a shared broker used by another client. Launch shared brokers through local Windows WMI as the same verified user, outside the caller's Job. The short-lived hidden launcher passes environment as data, without credentials in command lines or handoff files. The new service immediately owns its own Job and a finite readiness watchdog, then runs the broker in the same interpreter. Ordinary native workers retain atomic child Job containment. No scheduled task or installed Windows service is required.

4. **Share diagnostics through a local broker.** `tools/lsp/broker.py` owns one `DiagnosticsService`; existing STDIO MCP and --once hook entrypoints forward calls. An owner-private endpoint record, random bearer token, loopback socket, startup lock, lifetime lock and source/interpreter fingerprint prevent accidental endpoint reuse or duplicate launch. Initial cache limit is four backends; backend and service idle expiration default to 300 seconds. Calls carry their own workspace-root context. Pool keys retain root/language/Delphi project/registry/settings identity, while session/agent journal ownership stays outside the shared backend. Operations and document revisions are serialized. Hook claims transfer to service ownership, so thin-client exit cannot release active work. A stale source identity is explicit rather than silently using an old service.

5. **Route Serena by project.** A local shared adapter selects a project-specific native Serena backend for each client. `activate_project` changes that client's routing selection, not the shared worker's project. Compatible project configuration is part of identity; operations are serialized and idle workers retired. Preserve provisioning guards and all required semantic operations. A single globally mutable SerenaAgent was rejected.

6. **Preserve appropriate existing reuse.** Graphify's authenticated identity-checked shared service remains the query backend, with explicit project_path and bounded graph cache. Nuphus retains lazy isolated browser instances where its native protocol does not provide independent contexts; native/tool startup becomes lazy where feasible, with owned cleanup. Desktop mutations must not race shared interactive state. Do not claim one universal browser instance without implementing context isolation.

7. **Reduce launcher residency.** Native MCP registration can target the adopted Python interpreter and authoritative source directly, avoiding persistent PowerShell layers. Execute compatible Python entrypoints in the existing interpreter where this preserves module/import and STDIO behavior. Keep a compatibility launcher for existing consumers and installation relocation, and test actual Windows pipes rather than substituting CRT exec.

## Risks / Trade-offs

- Explicit refresh means retained graphs may be stale → expose policy, preserve exact source/coverage checks and explicit indexing.
- OS allocation limits may fail large requests → preserve old graph, show a resource failure, reduce generated inputs, and never retry unbounded.
- A shared service crash affects compatible clients → owned cleanup, finite error paths and subsequent bounded recovery; journals remain durable.
- Document/settings races could corrupt semantic results → canonical keys, request serialization, revision checks and concurrent-root regression tests.
- Package or source updates can strand a warm service → fingerprint verification, idle retirement and documented restart rather than mutating a running process in place.
- Existing tools launched outside the kit can bypass its worker supervisor → document that enforcement boundary; migrate shared watch settings consistently and verify Codex's delivered entrypoints.

## Migration Plan

Preserve baseline logs, versions, configuration and process identities. Land and validate process containment and broker fixtures first. Activate watch containment and approved exclusions before the large acceptance index. Register optimized global launchers through the installer; verify new clients outside the checkout. Clean only confirmed old PSES trees using checked identities. Run bounded real indexing and MCP/LSP probes, record resource measurements, then close tasks. Rollback restores owned configuration only when unchanged since activation, retires owned services, and retains previously committed native indexes and unrelated user work.
