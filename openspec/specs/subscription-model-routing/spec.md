# Subscription model routing

## Purpose

Make subscription-backed external models available in the official Codex CLI across projects, including deterministic model assignments for named subagents and a recoverable global installation.

## Requirements

Named-role requirements below describe the compatibility baseline. The accepted
transition is owned by the archived
[exact model/effort change](../../changes/archive/2026-09-20-orchestrate-subscription-agents/specs/subscription-model-routing/spec.md)
and [delegation contract](../agent-delegation/spec.md#requirements).
Its pending migration and acceptance are not established by retaining this baseline.

### Requirement: Subscription-backed Grok access
The kit SHALL provide xAI subscription authentication for the user's SuperGrok Heavy account and expose models actually available through that account on the native Codex Grok profile. A Grok request SHALL use the subscription route without requiring an xAI pay-as-you-go API key. Credentials MUST remain outside tracked reusable artifacts and MUST NOT be printed in diagnostics. After the Responses spike passes, Grok MUST NOT depend on OpenCodex, a local OpenAI-compatible proxy, or a Chat Completions translator.

#### Scenario: Grok runs as the main model
- **WHEN** the authenticated user selects an available Grok model through the native xAI profile of the ordinary global Codex CLI from another project
- **THEN** the request completes through xAI subscription authentication and the actual provider and model can be verified without disclosing credentials

#### Scenario: Subscription access is unavailable
- **WHEN** xAI authentication or quota prevents a request
- **THEN** the failure is visible and the request is not silently sent to a paid API route or a different model

### Requirement: Exact model and effort assignment

The kit SHALL provide native assignment parameters for an enabled provider profile and its supported reasoning effort, with lead and executor bindings selected through kit orchestration configuration while explicit user profile selection retains its native precedence. The supported heterogeneous mode SHALL deliver the assigned task and actual tool results through the configured profiles. Each active conversation SHALL appear on its own visible titled terminal surface - a dedicated tab, a pane or a window - with its effective identity; simultaneous on-screen tiling SHALL NOT be required. Autonomous recovery SHALL create a distinct visible task-level reassignment with a newly verified binding; an exact request SHALL NOT be silently relabeled or rerouted. Cross-provider continuation SHALL use sufficient visible task context and preserved artifacts, without assuming another provider can consume encrypted reasoning or compaction state.

#### Scenario: GPT delegates a review to Grok middle
- **WHEN** the lead assigns a review to the configured Grok executor profile from outside the harness checkout
- **THEN** the child receives the task, uses the selected profile's model, exercises an appropriate local tool and returns its findings

#### Scenario: The role's model is unavailable
- **WHEN** an explicitly assigned model cannot serve a delegated request
- **THEN** the failure remains observable and any authorized fallback is a distinct, visible assignment to a capable configured profile with preserved partial work

#### Scenario: A saved heterogeneous child needs recovery
- **WHEN** a restored task must continue a child whose provider binding cannot be verified
- **THEN** the runtime creates a fresh verified binding using the visible handoff and does not blindly resume it under an inherited model

### Requirement: Direct reusable configuration and private state
Reusable native profile, catalog and helper sources SHALL be read directly from repository sources through supported references or links. OAuth tokens, runtime logs, process state and installation recovery records SHALL remain on the host. Source links SHALL survive supported non-secret configuration writes. The kit SHALL NOT keep a linked OpenCodex `config.json` or role directory as the live Grok transport.

#### Scenario: Configuration is edited through the proxy
- **WHEN** a supported non-secret native profile or catalog setting is saved
- **THEN** the active configuration still resolves to the repository source and credentials remain outside that source

### Requirement: Global reversible lifecycle
The kit installation lifecycle SHALL connect the native Grok profile, Z.AI profile and token helper without installing OpenCodex, starting a subscription proxy, or injecting `openai_base_url` to localhost. It SHALL support Check, Update, Recover and Disconnect for those native pieces. Preview and Check MUST NOT install packages or mutate settings. Repeated installation SHALL be idempotent. Failures SHALL preserve recovery information and MUST NOT report a partial activation as complete. Disconnect SHALL remove owned Grok/Z.AI profile wiring while preserving unrelated configuration, native GPT, OpenCode authentication and stored subscription credentials. After a successful Responses spike, Disconnect and Update MUST also remove any remaining owned OpenCodex task, proxy injection and package connection.

#### Scenario: Install and restart
- **WHEN** installation completes and a new ordinary Codex session starts outside the checkout
- **THEN** native GPT works without a proxy, and the xAI profile can reach Grok after login without a background OpenCodex process

#### Scenario: Installation fails or is interrupted
- **WHEN** a lifecycle step fails after changing a managed connection
- **THEN** the prior usable state is restored or a precise recoverable pending state identifies the original cause and next recovery action

#### Scenario: Preview and repeat installation
- **WHEN** the user previews installation or checks it, then repeats a successful installation
- **THEN** preview/check make no persistent changes and repeat installation creates no duplicate links, services or provider entries

#### Scenario: Disconnect and reconnect
- **WHEN** the user disconnects and subsequently reconnects the integration
- **THEN** native Codex works after disconnect, no active kit path requires OpenCodex, and reconnect restores the native Grok/Z.AI profiles without deleting stored credentials

### Requirement: Runtime memory containment
Managed login, token-helper and administrative commands SHALL have bounded elapsed time and SHALL NOT leave uncontrolled child processes. Browser login SHALL work without reading unavailable terminal input. The kit SHALL NOT require a Windows Job around an OpenCodex proxy after retirement. Isolated fixtures MUST NOT target a live global proxy.

#### Scenario: Runtime exceeds its resource allowance
- **WHEN** a managed login or helper exceeds its time or resource allowance
- **THEN** the limit is enforced, owned descendants are cleaned up, and the failure is observable with a documented recovery action

#### Scenario: Login has no interactive stdin
- **WHEN** the delivered browser or device login runs with closed stdin
- **THEN** it waits for the callback without retrying terminal reads, or terminates within its time allowance

### Requirement: Existing consumers remain compatible
Integration SHALL preserve the existing harness launcher, instructions, skills, MCP and diagnostic connections. It SHALL preserve OpenCode configuration and authentication; shared OAuth refresh ownership MUST NOT be introduced without evidence that both clients remain usable. Explicit user overrides SHALL retain native Codex precedence. Z.AI SHALL remain available through `codex --profile zai` on the existing Responses key path. Native GPT SHALL remain the default ordinary session after OpenCodex removal.

#### Scenario: Existing kit and OpenCode coexist
- **WHEN** the native Grok profile is activated for Codex
- **THEN** the original harness entry point and global capabilities remain available and OpenCode's existing configuration and authentication are preserved

### Requirement: Verifiable delivery and bounded extension claims
Completion SHALL include evidence from the real global Codex entry point outside this repository: the Responses spike, `codex --profile xai` with a tool-using Grok turn, ordinary `codex` without proxy injection, Z.AI profile independence, OpenCodex task/package absence, representative auth failure, and rollback/disconnect. Deterministic tests SHALL cover ownership and recovery boundaries where destructive effects cannot safely be exercised on the live installation. Documentation SHALL distinguish providers actually verified from providers supported only by configuration mechanisms.

#### Scenario: Delivery evidence is reviewed
- **WHEN** the implementation is reported complete
- **THEN** each required behavior has recorded evidence, substitute-environment limits are identified, and all associated implementation tasks reflect actual completion

### Requirement: Native Codex Grok profile
The kit SHALL expose SuperGrok Heavy through an explicit native Codex profile (`codex --profile xai` or the documented equivalent) that uses Codex custom-provider Responses, host-private OAuth credentials, and a bounded model catalog. Ordinary sessions SHALL NOT require that profile. Grok MUST use the user's existing xAI subscription and MUST NOT require an xAI pay-as-you-go API key. Credentials MUST remain outside tracked reusable artifacts and MUST NOT be printed in diagnostics. A native helper MAY refresh and emit a short-lived access token for Codex provider auth.

#### Scenario: User starts Grok through the profile
- **WHEN** an authenticated user runs the documented xAI profile from another project
- **THEN** Codex talks to xAI over Responses with subscription OAuth, `grok-4.6` is selectable, and native session evidence identifies that provider and model without disclosing tokens

#### Scenario: Ordinary Codex stays on GPT
- **WHEN** the user starts ordinary `codex` without the xAI profile or an explicit Grok override
- **THEN** the session uses native GPT/Astra routing and does not send traffic to an OpenCodex or other kit-owned proxy

#### Scenario: Auth helper refreshes a token
- **WHEN** Codex requests a bearer token through the configured provider auth command
- **THEN** the helper prints only a current access token to stdout, refreshes from the private store if needed, and writes no secret into tracked source or diagnostics

### Requirement: Responses spike gate for Grok
Dependent Grok routing, profile installation and OpenCodex removal SHALL NOT proceed until a bounded live probe proves that subscription OAuth can complete a Codex-shaped Responses request for `grok-4.6` at `https://api.x.ai/v1`, including at least one tool call. The probe MUST use the subscription scopes, MUST NOT use a pay-as-you-go key, and MUST retain status, endpoint and model identity without tokens. If the probe fails, OpenCodex SHALL remain connected and this change SHALL stay incomplete.

#### Scenario: Responses probe succeeds
- **WHEN** the probe authenticates with subscription OAuth and receives a successful `grok-4.6` Responses result with a tool call
- **THEN** native profile work and OpenCodex retirement are authorized

#### Scenario: Responses probe fails
- **WHEN** the endpoint rejects Responses, OAuth, tools, or `grok-4.6` on the subscription
- **THEN** the failure remains visible, OpenCodex is not removed, and no pay-as-you-go API route is substituted

### Requirement: Independent xAI executor transport startup
An observed executor whose resolved provider is xAI SHALL prepare and verify its installed compatibility transport before starting its app-server conversation. It MUST NOT require an interactive xAI session or a model warmup request. Preparation SHALL retain the existing OAuth credential helper and shim build-identity lifecycle. The shared shim MUST NOT be owned by an individual executor's app-server, preflight or tool cleanup Job, and MUST NOT retain the caller's output pipes. Other providers SHALL NOT start the xAI shim. Preparation failures SHALL be reported before submitting the assignment, with the failed transport component identified.

#### Scenario: Cold xAI executor startup
- **WHEN** an xAI executor starts while its compatibility shim is absent
- **THEN** the installed shim becomes ready before the conversation starts without a separate interactive session

#### Scenario: Another executor already uses the shim
- **WHEN** another xAI executor starts or one app-server conversation ends
- **THEN** the matching installed shim is reused and individual app-server cleanup preserves the shared transport

#### Scenario: Transport preparation fails
- **WHEN** the selected shim manager is unavailable or fails to become ready
- **THEN** the executor reports the transport preparation failure without starting a model turn or falling back to another provider

#### Scenario: Temporary preflight starts the shim
- **WHEN** a temporary preflight or tool invocation starts the shared shim and its cleanup Job subsequently ends
- **THEN** the shim remains ready for other sessions and the caller's output capture reaches EOF

#### Scenario: Executor uses a different provider
- **WHEN** an executor starts with a resolved provider other than xAI
- **THEN** its startup does not prepare an xAI shim
