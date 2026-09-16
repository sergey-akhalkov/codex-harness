## MODIFIED Requirements

### Requirement: Bounded automatic runtime recovery
Owned subscription login and token-helper failures SHALL remain observable without an infinite restart loop. The kit SHALL NOT run a background OpenCodex proxy or Task Scheduler host for Grok after the Responses spike passes. Explicit disconnect MUST leave native Grok/Z.AI profile wiring stopped or removed as documented. Stored credentials MUST remain unless the user deletes them.

#### Scenario: One runtime crash
- **WHEN** a token helper or login command fails
- **THEN** the failure is retained in private evidence, Grok is not silently rerouted through a proxy, and the next explicit login can retry

#### Scenario: Repeated failure
- **WHEN** authentication keeps failing
- **THEN** the kit does not spawn an unmanaged restart loop and retained logs identify the failed attempts

#### Scenario: Intentional disconnection
- **WHEN** the user disconnects the subscription integration
- **THEN** owned native profile wiring is removed, no OpenCodex task reconnects it, and stored credentials are preserved

### Requirement: Non-disruptive recovery policy activation
Lifecycle updates for native Grok/Z.AI profiles SHALL be applicable without stopping unrelated Codex sessions or mutating foreign configuration. Ownership conflicts and unfinished lifecycle operations MUST prevent unsafe writes. Preview MUST be read-only; interrupted updates MUST be recoverable. The installer SHALL NOT require applying a proxy-process restart policy.

#### Scenario: Apply while a session uses the proxy
- **WHEN** native subscription wiring is updated while another Codex session is open
- **THEN** that session is not forced onto a proxy, and subsequent checks accept the new native definition

#### Scenario: Interrupted or conflicting activation
- **WHEN** a policy or profile update is interrupted or another writer changes ownership state
- **THEN** recovery restores the recorded state or reports a conflict while preserving foreign changes and recovery evidence

### Requirement: Reusable and verified global recovery
Native Grok/Z.AI profile connection SHALL be maintained in reusable kit sources and applied by new installation and update. Acceptance MUST verify the xAI profile through a fresh global Codex session outside this checkout after OpenCodex is absent. It MUST NOT deliberately crash a live user session and MUST NOT keep a global proxy as a recovery target.

#### Scenario: Global delivery
- **WHEN** this change is declared complete
- **THEN** no owned OpenCodex task is required for Grok, ordinary Codex uses native GPT, and an externally launched `codex --profile xai` session completes a tool-using Grok turn

