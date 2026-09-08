# Subscription model routing

## Purpose

Make subscription-backed external models available in the official Codex CLI across projects, including deterministic model assignments for named subagents and a recoverable global installation.

## Requirements

### Requirement: Subscription-backed Grok access

The kit SHALL provide xAI subscription authentication for the user's SuperGrok Heavy account and expose models actually available through that account. A Grok request SHALL use the subscription route without requiring an xAI pay-as-you-go API key. Credentials MUST remain outside tracked reusable artifacts and MUST NOT be printed in diagnostics.

#### Scenario: Grok runs as the main model
- **WHEN** the authenticated user selects an available Grok model through the ordinary global Codex CLI from another project
- **THEN** the request completes through xAI subscription authentication and the actual provider and model can be verified without disclosing credentials

#### Scenario: Subscription access is unavailable
- **WHEN** xAI authentication or quota prevents a request
- **THEN** the failure is visible and the request is not silently sent to a paid API route or a different model

### Requirement: Exact model assignment to named roles

The kit SHALL deliver a globally discoverable Grok middle role supporting review tasks and a documented method of assigning an enabled provider/model to another named role. Role selection SHALL honor the declared model and compatible reasoning settings while the parent can remain on its existing GPT model. The supported heterogeneous delegation mode SHALL deliver the actual task and tool results to the child.

#### Scenario: GPT delegates a review to Grok middle
- **WHEN** a GPT main agent invokes the named Grok middle for a review from outside the harness checkout
- **THEN** the child receives the assigned task, uses the selected Grok model, exercises an appropriate local tool, and returns its findings to the parent

#### Scenario: The role's model is unavailable
- **WHEN** the explicitly assigned model cannot serve the delegated request
- **THEN** the failure remains observable and no fallback or alias silently substitutes another model

### Requirement: Direct reusable configuration and private state

Reusable proxy settings and agent definitions SHALL be read directly from repository sources through supported references or links. OAuth tokens, catalogs, runtime logs, process state and installation recovery records SHALL remain on the host. Source links SHALL survive supported proxy configuration writes.

#### Scenario: Configuration is edited through the proxy
- **WHEN** a supported non-secret proxy setting is saved
- **THEN** the active configuration still resolves to the repository source and credentials remain outside that source

### Requirement: Global reversible lifecycle

The kit installation lifecycle SHALL install or reuse its declared OpenCodex dependency, connect the proxy globally, provide a hidden Windows background startup, and support Check, Update, Recover and Disconnect. Preview and Check MUST NOT install packages or mutate settings. Repeated installation SHALL be idempotent. Failures SHALL preserve recovery information and MUST NOT report a partial activation as complete. Disconnect SHALL remove owned routing and connections while preserving unrelated configuration and authentication.

#### Scenario: Install and restart
- **WHEN** installation completes and the managed background process is restarted
- **THEN** the proxy becomes ready and a new ordinary Codex session outside the checkout retains the configured provider and role access

#### Scenario: Installation fails or is interrupted
- **WHEN** a lifecycle step fails after changing a managed connection
- **THEN** the prior usable state is restored or a precise recoverable pending state identifies the original cause and next recovery action

#### Scenario: Preview and repeat installation
- **WHEN** the user previews installation or checks it, then repeats a successful installation
- **THEN** preview/check make no persistent changes and repeat installation creates no duplicate links, services or provider entries

#### Scenario: Disconnect and reconnect
- **WHEN** the user disconnects and subsequently reconnects the integration
- **THEN** native Codex works after disconnect, no active kit role requires a removed route, and reconnect restores the intended subscription configuration without deleting stored credentials

### Requirement: Runtime memory containment

Managed OpenCodex processes and their descendants SHALL start inside a Windows-enforced job memory limit. Login and administrative commands SHALL also have bounded elapsed time. A failed allocation or timeout MUST NOT leave an uncontrolled child process or silently trigger endless restarts. Browser login SHALL work without reading unavailable terminal input.

#### Scenario: Runtime exceeds its resource allowance
- **WHEN** a managed process or its child attempts to exceed the configured memory limit, or a bounded command times out
- **THEN** the limit is enforced without unbounded host allocation, owned descendants are cleaned up, and the failure is observable with a documented recovery action

#### Scenario: Login has no interactive stdin
- **WHEN** the delivered browser login runs with closed stdin
- **THEN** it waits for the browser callback without retrying terminal reads, or terminates within its time allowance

### Requirement: Existing consumers remain compatible

Integration SHALL preserve the existing harness launcher, profile, instructions, skills, MCP and diagnostic connections. It SHALL preserve OpenCode configuration and authentication; shared OAuth refresh ownership MUST NOT be introduced without evidence that both clients remain usable. Explicit user overrides SHALL retain native Codex precedence.

#### Scenario: Existing kit and OpenCode coexist
- **WHEN** OpenCodex is activated for Codex
- **THEN** the original harness entry point and global capabilities remain available and OpenCode's existing configuration and authentication are preserved

### Requirement: Verifiable delivery and bounded extension claims

Completion SHALL include evidence from the real global Codex entry point outside this repository, exact named-role delegation, subscription routing, representative failure paths, background restart and rollback. Deterministic tests SHALL cover ownership and recovery boundaries where destructive effects cannot safely be exercised on the live installation. Documentation SHALL distinguish providers actually verified from providers supported only by configuration mechanisms.

#### Scenario: Delivery evidence is reviewed
- **WHEN** the implementation is reported complete
- **THEN** each required behavior has recorded evidence, substitute-environment limits are identified, and all associated implementation tasks reflect actual completion
