## Context

See proposal.md for motivation. `tools/kit.psm1` owns link inventory and lifecycle. `install.ps1` dispatches core, combined and subscription modes. Native CLI 0.153.4 provides `config/read` with `includeLayers`/`cwd`, config origins and layer metadata, plus `skills/list`. Existing consumer tests use stdio app-server. The diagnostic must remain useful when installation links or TOML are damaged.

## Goals / Non-Goals

**Goals:** One explicit model-free diagnostic, native source resolution, aggregated recoverable findings, reusable global installation.

**Non-Goals:** Full doctor, automatic repairs, model regressions, project command storage, active-session introspection and server-revision attestation. These remain independently ranked audit candidates. No claims of measured weekly savings.

## Decisions

1. Add an explicit Diagnose switch to Check and a direct linked command. Bypass activation preflight for this read path so one broken link does not hide other findings. Ordinary Check remains the deeper installation/protocol check.
2. A PowerShell module owns short-lived stdio native app-servers with explicit CODEX_HOME, a shared finite request deadline and finally cleanup. Resolve the registered executable or known npm vendor layout without searching project PATH. Only initialize, config/read, configRequirements/read and skills/list are used; no thread/turn/tool calls.
3. CLI 0.153.4 app-server rejects file profiles. Read base layers for the target directory, then native-parse the profile via a temporary home whose config.toml links to the original profile. Extract only its user layer; insert it after the base user layer in lowest-first precedence (the RPC returns highest first). Compute only documented simple preferences and mark every winner inferred-from-native-layers, never native effective-profile evidence. Unknown versions, managed requirements or profile changes affecting discovery/trust/skills return incomplete; uncertain winners are withheld. Do not emulate TOML with regex. Suppress prompt bodies and raw native errors. Skills come from the native base consumer; deduplicate identical source paths before flagging duplicate names.
4. Inspect each recorded link independently and compare against current inventory to catch a newly added but unregistered command. Report pending transactions without trying recovery. Expose source metadata, never arbitrary file contents.
5. Keep running-session and MCP/LSP freshness explicitly unknown. A new consumer is evidence about new loading only. The existing Check remains the protocol-health entry point.

## Risks / Trade-offs

- Experimental native schema changes → inspect the generated schema, test real CLI and return incomplete on unsupported contracts.
- Secrets in invalid TOML errors → do not forward raw errors, stderr or entire configs; sentinel tests cover failure paths.
- Side effects of app-server → never start a thread; verify user-state fingerprints; document incidental native caches/logs.
- Existing global proxy controls this session → never restart or disconnect it during acceptance; update core inventory through Invoke-HarnessInstall with IncludeCodeTools to preserve installed hook links, after reviewing a preview.
- A report cannot prove freshness of existing servers → explicit unknown rather than a guessed stale/clean label.

## Migration Plan

Run isolated tests, update the global core links through the existing installer, invoke the global command outside the checkout, and run normal core/consumer regression checks. Keep sources linked. Disconnect rollback is tested only with isolated homes. Archive after every task has evidence; commit and push all requested repository changes after reviewing the complete candidate.
