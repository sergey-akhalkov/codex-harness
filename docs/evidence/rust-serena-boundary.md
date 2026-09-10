# Serena 1.7.0 external boundary

2026-09-10; bounded investigation for [migration task 3.2](../../openspec/changes/migrate-harness-to-rust/tasks.md). **Configuration isolation has a supported implementation path. A complete replacement of the shared-session monkeypatch is not yet proven:** native HTTP provides separate prompt sessions, but the shipped CLI does not protect its listener with authentication; stdio provides only one native session. Keep task 3.2 and the installed adapter unchanged.

## Identity and scope

Inspected the installed external `serena-agent` 1.7.0 at `C:/Users/noilw/AppData/Roaming/uv/tools/serena-agent/Lib/site-packages` (paths below are relative to this directory), with its declared MCP dependency `mcp==1.28.1`. Direct CLI: sibling `../../Scripts/serena.exe` relative to `site-packages`. No Python monkeypatch was loaded in the experiment. Six boundary files matched their installed wheel `RECORD` hashes; this verifies local consistency, not independent package authenticity:

| File | SHA-256 |
| --- | --- |
| `serena/config/serena_config.py` | `8277D32A0F3B6F6760240351129792C8E24BB4631E461789E0B911204F468218` |
| `serena/project.py` | `19B779AF83FDC3F9C6480FDED0DECF9C326C4E6AB7419FEEECBE5205A1999B64` |
| `serena/mcp.py` | `DE30CF112EE6FBCC68691716EE0B4B53CE5846AE1CA1E49AA0FE02BB1C31C0E0` |
| `serena/cli.py` | `1620577816B52632F611983D6D2044F228C049B6C8D931EF20B54CD1E113081A` |
| `serena/tools/tools_base.py` | `C6547239C9BBA58A761ADD6F9871FE3CA8D97A6043A2013E3476873CF919E2C5` |
| `solidlsp/dependency_provider.py` | `475A91740A37B5BF3E0813AD71D52E071A811809324C180E3C5860619EDE9B55` |

Repository HEAD was `1ba176c9bf8926795e734bb76e8a6b4d38459dc3`, with concurrent dirty work. Repository inspection was limited to the two seams in [serena_entry.py](../../tools/code-tools/serena_entry.py), relevant routing in [serena_broker.py](../../tools/code-tools/serena_broker.py), and planning records. Dependency discovery/metadata implementation was outside this investigation. Serena MCP confirmed the intended repository and Python/Rust support; reading both wrapper definitions through `find_symbol` succeeded. The already-reported stale CBM graph was not used.

## Verified configuration contract

- `SERENA_HOME` redirects user configuration/state. `project_serena_folder_location` redirects **the whole project data folder**, including `project.yml`, local overrides, memories and caches. These are documented settings, independently checked in installed `serena_config.py:1375–1421` and the [official configuration documentation](https://oraios.github.io/serena/02-usage/050_configuration.html#per-project-serena-folder-location).
- The configured **directory must already exist**. Otherwise Serena falls back to `<project>/.serena` when that directory exists. A fixed absolute private directory is suitable for a worker permanently bound to one canonical root. `$projectFolderName` alone is not a collision-safe identity for unrelated roots with the same basename.
- `ProjectConfig.load` (`serena_config.py:683–734`) loads `project.yml`, applies sibling `project.local.yml` using a top-level `dict.update`, and may save completed defaults. Global loading (`1027–1099`, `_migrate_out_of_project_config_file`) can migrate file-valued registered projects. Therefore copying the shared global config unchanged into a private home is insufficient: normalize registrations to validated directory roots, and ensure every loaded project resolves to owned configuration before launching. Do not link the entire private data directory back to shared `.serena`.
- `project.py:519–535` copies global `ls_specific_settings`, then performs **shallow language-key replacement** with project settings, only for trusted projects. `trusted_project_path_patterns: []` suppresses all project LS-specific settings. Merely making a project untrusted loses its semantic overrides unless the Rust boundary first places the effective settings in private global config.
- Concrete counterexample: global `python: {ls_base_cmd: [selected], ls_args: [--stdio]}` plus trusted project `python: {initializationOptions: {...}}` removes both command keys. The Python provider then takes its `uvx` path. A project `ls_base_cmd` directly overrides the selected command. Neither case is prevented by an ordinary global command override.
- `solidlsp/dependency_provider.py:86–119` uses a list-valued `ls_base_cmd` without `_create_default_base_command`; explicit list-valued `ls_args` also bypasses provider-specific command construction. `ls_extra_args` is appended. Non-list base commands are ignored and can restore installer selection. `ls_path` has lower priority than a valid base-command list. Require nonempty, typed argv and validated existing executable/entrypoint paths; do not rely on upstream's permissive validation.
- The accepted Python providers (`pyright_server.py:42–51`, `basedpyright_server.py:42–51`) use `LanguageServerDependencyProviderUvx`; Rust uses its `DependencyProvider` (`rust_analyzer.py:189–212`). The explicit-command branch avoids their default acquisition path. Semantic initialization options still flow through `solidlsp/initialize_params.py:72–81`. This is source evidence of provider selection, **not a run proving absence of every possible download**.
- Current `restart_language_server` has no settings arguments and recreates the manager. However, `activate_project` can load another configuration, optional `query_project` can reach another project/server, project activation can execute `activation_command`, and future tools may add other paths. `ls_priorities` is detection preference, not a language allowlist. A version/schema-gated Rust broker must mediate these entries; config presence is not an installation-denial capability.

## Verified session and transport contract

`serena start-mcp-server --help` exposes stdio/SSE/streamable HTTP, host/port, context/modes and UI controls. `--project-file` is a deprecated alias for project name/root selection, not an arbitrary overlay. The factory constructs one `SerenaAgent` (`serena/mcp.py:309–391`) and retains it across HTTP connections (`396–419`).

`Tool.apply_ex` (`tools_base.py:339–377`) computes the prompt session key from `id(mcp_ctx.session)` and injects it itself. It does not interpret broker client metadata as a session selector. Thus changing JSON-RPC IDs, client metadata, or supplying a `session_id` argument cannot split an unmodified stdio worker into native sessions.

MCP's stateful HTTP manager creates a transport and server run per session (`mcp/server/streamable_http_manager.py:264–303`). That gives separate Serena prompt state **but not separate active projects or modes**. The existing Rust port still needs a fixed-project/configuration worker key and client-specific routing before activation; sending another root to an existing shared agent violates isolation.

The installed FastMCP constructor defaults `auth=None`; Serena supplies neither token verifier nor authorization provider. FastMCP requires one of those when authentication is enabled (`mcp/server/fastmcp/server.py:147–224`). A `FASTMCP_AUTH` setting cannot supply that missing verifier. Its loopback Host/Origin checks are DNS-rebinding protection, not caller authentication. CLI launch exposes neither an inherited/private socket nor Uvicorn UDS/fd options (`753–789`). An authenticated Rust reverse proxy alone leaves the upstream TCP port directly reachable. Random port/session IDs do not fix unauthenticated session creation.

Serena's prompt dictionary (`agent.py:292–314`) retains stringified object identities without a per-session removal hook. HTTP transport deletion does not remove this agent state. The HTTP manager supports an optional idle timeout, but FastMCP's construction does not set it (`fastmcp/server.py:954–962`). Bound both active sessions and lifetime session churn; eventual worker retirement is needed unless upstream exposes cleanup. Object-ID reuse after churn is a source-derived risk, not reproduced here.

## Owned execution evidence

Raw local evidence: `%TEMP%/serena-boundary-3e19f2f9b7e045f590d65ec14a538ff7/` (`probe.json`, `result.json`, request-result `.txt` files, stdout/stderr). Invocation from that root:

```text
SERENA_HOME=<root>/home
<installed>/Scripts/serena.exe start-mcp-server --transport streamable-http
  --host 127.0.0.1 --port 51432 --context codex --project <root>/owned-a
  --enable-web-dashboard false --enable-gui-log-window false --open-web-dashboard false
```

Private config selected `<root>/data/$projectFolderName`, empty trusted paths, and only `initial_instructions`, `get_current_config`, `activate_project`. Two owned project configs had `language_servers: []`; no language-server process or package acquisition was needed. PowerShell issued standard MCP HTTP requests, without creating an executable probe script.

| Oracle | Observed result |
| --- | --- |
| Two `initialize` requests, no Authorization header | Both HTTP 200; distinct MCP session IDs |
| A: first and second `initial_instructions`; B: first call | `<active-project>` present / absent / present; text lengths 4966 / 4667 / 4966 |
| Prompt-session identity in native logs | A repeated the same object identity; B had a different one |
| B activates owned project B, then A reads configuration | A reports `Active project: private-b`; native HTTP does not isolate projects |
| Competing `<root>/owned-a/.serena/project.yml` | Private config won; shared sentinel SHA-256 remained `D78C5AEE793575DDEE13BED5E98A2C010EDD00636A7527678323DBCEEE4B92EA` |
| Cleanup | Owned launcher tree stopped; PID 21028 absent; port 51432 had zero listeners |

Mode-tag presence was initially checked but was not a distinguishing oracle: both modes remain in repeated prompts in this scenario. The project-prompt oracle above establishes the actual difference. This experiment proves configuration routing, native prompt-session separation and the two isolation counterexamples; it does not satisfy task 3.2's real semantic acceptance.

## Minimal proposed boundary and exact remaining interface

1. Rust owns admission and the existing bounded worker pool. Key workers by canonical project root, relevant config/local overrides, context/modes/templates, selected command identities and protocol compatibility. Keep caller project selection/removals outside a shared agent; retain current capacity, deadlines and ownership rules.
2. Build private **runtime state** for one fixed root: precreated project data directory, normalized registrations and effective semantic settings. Preserve project/local precedence, then replace only process-selection fields with accepted Rust/Python argv. A defensible layout puts these final LS settings in private global config with empty trusted paths and no project LS override. Validate `language_servers` against the accepted provider IDs before startup; never auto-detect missing configuration. Handle activation commands explicitly rather than silently changing their behavior. Keep source-owned kit configuration directly linked; this runtime projection is not a deployment copy of kit data. Memory/cache relocation and custom prompt preservation also need explicit integration checks.
3. Use the absolute installed CLI and owned environment, disable dashboard/GUI, keep external package files unchanged. Revalidate inputs before worker creation/restart and reject unknown versions/tools rather than trusting future provisioning paths. A missing/incompatible adopted dependency must fail before Serena starts or fail its explicit launch, with no default-provider retry.
4. **Unmet interface:** retaining both native per-client prompt sessions and the current private authenticated shared-worker boundary requires either (a) an upstream-supported authenticated/private multi-session listener, including a usable verifier/secret or private socket entry point; or (b) an upstream-supported bounded client-session selector and close operation over private stdio. Neither is supplied by this Serena 1.7.0 CLI. OS-enforced backend network isolation could be another Rust-controlled solution, but no usable non-global implementation was established here. Per-client full Serena processes, plaintext loopback behind a proxy, or an owned Python launcher do not establish the accepted replacement.

A Rust implementation of the two currently session-aware prompt tools is a separate possible research path, not a verified native interface: arbitrary Jinja prompt overrides and mutable memory/onboarding content make cached-response replay insufficient without further equivalence proof.

### WFP lifetime question

A subsequent bounded Windows policy consultation found a potential rule shape,
but no complete runtime proof. At ALE connect and receive/accept layers, WFP can
match the exact loopback address/port, executable AppID and owning user for both
IPv4 and IPv6. AppID identifies an executable path, not a process instance; the
actual socket-owning interpreter must be identified. The
[documented conditions](https://learn.microsoft.com/en-us/windows/win32/fwp/filtering-conditions-available-at-each-filtering-layer)
and [identity definitions](https://learn.microsoft.com/en-us/windows/win32/fwp/filtering-condition-identifiers-)
do not provide a simple ALE process-ID condition at those layers.

Dynamic protection alone leaves an unproved crash-ordering path: closing the
[dynamic WFP session deletes its objects](https://learn.microsoft.com/en-us/windows/win32/api/fwpmu/nf-fwpmu-fwpmengineopen0),
while terminating and awaiting the owned backend is a separate operation. No
documented ordering was established that makes the listener unreachable before
filter deletion after an abrupt helper exit. This is a design concern, not a
reproduced exploit. An owned port quarantine retained until verified backend
teardown is a possible alternative to investigate through the native recovery
lifecycle. BFE access rights, policy arbitration, service-restart behavior,
startup/port-reuse races and actual unauthorized-client denial remain unverified.
No firewall rule or service was changed by the consultation; WFP is not yet an
accepted replacement boundary.

Next acceptance after choosing a safe session boundary: actual Rust and Python semantic calls in two owned roots; selected argv plus semantic-option retention; project/local override and missing-overlay counterexamples; missing/incompatible dependency failures; no acquisition at startup/restart/activation; unauthorized direct-backend access; concurrent clients, cancellation, churn/eviction and cleanup; unchanged shared configuration and correct memory behavior. No executable, package, global setting or task checkbox was changed in this investigation.
