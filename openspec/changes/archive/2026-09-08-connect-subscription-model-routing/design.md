## Context

See [proposal.md](proposal.md) for the accepted outcome. Codex CLI 0.153.4 and Node 24.18.1 are installed. Ordinary `codex` resolves to the kit launcher and selects the linked harness profile. That profile currently selects GPT-6 Astra at xhigh, without overriding provider or base URL. The native CLI reports multi_agent enabled and multi_agent_v2 disabled. The user's existing OpenCode xAI authorization is OAuth; only its field names were inspected, never its values.

OpenCodex npm 2.44.0 identifies source commit `07b48da8fd63881e848d26e0bd50087864f5573e`. Its xAI OAuth route uses `https://cli-chat-proxy.grok.com/v1`. Loopback Codex integration uses a marker-owned root `openai_base_url` and a generated model catalog, preserving the native OpenAI provider identity. Its atomic writer resolves symlinks before replacement.

## Goals / Non-Goals

**Goals:** preserve official Codex and the direct-source kit architecture; prove one production subscription and one exact role before completing global lifecycle delivery; keep configuration ownership and rollback explicit.

**Non-Goals:** change OpenCode, implement a new model gateway, enable experimental encrypted-v2 task recovery, pool extra accounts, or claim activation of unspecified subscriptions.

## Decisions

1. Use the published pinned OpenCodex package, with its bundled runtime, as an external dependency. The kit owns its integration source and lifecycle; it does not fork the proxy. Recheck actual CLI help and installed package code before relying on operational flags.
2. Bind to loopback with a fixed port and retain the harness launcher. After the observed Bun memory incident, require a tested Windows Job Object memory cap for the foreground process and its descendants before runtime activation. A kit-owned hidden scheduled task can invoke that bounded foreground runner; an upstream detached service does not inherit this protection. Do not install a second Codex shim. Configure only the Codex integration.
3. Link a non-secret repository JSON source to the host OpenCodex config; store auth and generated state in the normal host directory. Inspect persisted JSON after account setup to ensure no credential-bearing management settings enter the reusable source. Extend the existing installer with a cohesive proxy lifecycle component and ownership/recovery records; use existing core transaction mechanisms where applicable.
4. Prefer a separate browser OAuth authorization for OpenCodex. Do not copy or refresh OpenCode's token as a shortcut: refresh-token sharing can invalidate the working client. The user completes account login, while installation and independent checks continue.
5. Keep the main model GPT-6 Astra and forward its current ChatGPT authorization. Fetch Grok models using the authenticated account, then pin the actual chosen coding model in `grok_reviewer` with supported reasoning. Place reusable role source under `global/`; connect it so Disconnect cannot leave a globally active role whose proxy was removed. Never place OpenCodex-only fields in Codex role TOML.
6. Keep heterogeneous collaboration on v1. Set no fallback or alias that shadows the role's provider/model. V2 full-history inheritance and encrypted tasks make an unrestricted v2 promise invalid; no experimental recovery is necessary for the accepted result.
7. Check effective provider/role configuration through the real consumer, not only JSON syntax or health probes. A harmless fixture outside this repository supplies a unique task marker and a local file for tool verification. Proxy request metadata establishes provider/model; agent self-identification alone is insufficient.
8. The first isolated `ocx login xai` was interrupted when the user killed an oversized Bun process. System event 2004 confirms memory exhaustion; the terminated PID's command line was not captured. Investigate the pinned CLI's closed-stdin manual-input retry loop with bounded Node execution. Use the package's browser callback flow without manual stdin in the delivered login entry point, force independent OAuth, and open the browser outside the bounded child-process job. Do not patch the installed dependency or copy OpenCode credentials.

## Risks / Trade-offs

- OAuth refresh ownership → separate login and preservation checks for OpenCode.
- Unexpected model substitution or separate API billing → subscription-only xAI, explicit model identifiers, no fallback/alias override, and negative-route verification.
- Conflicting configuration writers or interrupted installation → bounded ownership, source links, retained recovery journal, conflict refusal rather than overwriting foreign edits.
- Proxy downtime affects routed Codex → hidden startup, readiness checks, restart verification and native restoration.
- Runtime memory exhaustion → a Windows-enforced job commit limit, bounded login/administrative commands, observable failure, no automatic retry storm, and native-route recovery if the managed process cannot run.
- New source roles could be discovered before routing is ready → stage/activate the role connection with the proxy lifecycle and remove owned connection on Disconnect.
- Search/vision sidecars can consume ChatGPT quota → document their actual configured state and verify tool behavior relevant to the accepted scope.
- Current hosted session and native CLI differ → launch fresh native consumers; do not restart unrelated running Codex hosts to refresh their catalogs.

## Migration Plan

Capture current configuration and connection identity. Acquire and inspect the pinned package. Prove OAuth and a harmless Grok request with isolated runtime settings where practical. Implement and test lifecycle integration, then activate through the same delivered installer. Verify ordinary main-model and named-role requests outside the checkout, existing kit discovery, OpenCode preservation, service restart, repeat install and controlled rollback. Retain useful redacted evidence and mark tasks only after their checks pass. On failure, complete independent safe work and retain the exact unfinished acceptance requirement.
