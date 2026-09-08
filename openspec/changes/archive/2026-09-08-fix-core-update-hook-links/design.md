## Context

See [proposal.md](proposal.md). The observed global installation retains MCP registration and runtime inventory but lacks both hook links and their installation records. The original encoded PreToolUse command exits 1 outside the checkout because `harness/bin/hook.ps1` does not exist. No known-good historical version is asserted.

## Goals / Non-Goals

**Goals:** preserve hook connectivity through the existing transactional core installer and restore the affected global installation.

**Non-Goals:** changes to diagnostic findings, timeouts, provider routing or dependency versions. No restart of the session transport.

## Decisions

Build the desired hook inventory when code tools are explicitly included OR prior validated installation state contains either hook connection. This reuses existing target, ownership, source-move and rollback checks. Merely exempting hooks from deletion would leave stale targets on relocation and omit them from future installation metadata. Inferring activation from arbitrary runtime/cache files would silently activate fresh core-only installs.

The damaged real installation has already lost its hook records. Repair it using the existing core module with explicit `IncludeCodeTools`, after preview and backup. This restores links through the supported transaction without invoking dependency activation or subscription lifecycle.

## Risks / Trade-offs

- Foreign replacement or concurrent installer → existing ownership checks and mutex preserve state.
- Diagnostic service unrelated to this defect → verify real error/correction through the installed command while retaining honest unavailable reporting.
- Open sessions cache commands → restoring the same launcher path repairs subsequent calls; verify the identical command before and after.

## Migration Plan

Keep baseline source and installation metadata in local verification storage, pass the isolated installer regression, preview the global link repair, apply it and exercise installed hooks from owned temporary workspaces. Preserve pre-existing work and services. Rollback uses the saved state and exact owned link identities; source rollback alone must not remove restored hooks.

Global validation initially reported an outdated diagnostic source identity. The initial hypothesis was a stale service; an authenticated retirement and subsequent scoped recovery were attempted. Restart reproduced the mismatch with the same identity. Comparing effective environments then proved that command hooks omitted `HARNESS_CODE_TOOLS_REGISTRY`, while the MCP launcher passed the installed path. Setting that path alone exactly reproduced the endpoint's identity. The hook now passes the absolute inventory path it actually reads; broker code remains unchanged.

The 30-second retirement attempt remained pending and subsequent authenticated status still showed two leased retired Markdown backends. Their child processes were hours old. Recovery targeted only that retiring broker after verifying its receipt PID/start time, service command and separation from the control process ancestry. One PowerShell descendant survived its Job cleanup and was separately terminated after verifying PID/start time and its exact owned session-details path. The original retirement and recovery outcomes are retained; they did not solve the registry mismatch and are not claimed as its fix.
