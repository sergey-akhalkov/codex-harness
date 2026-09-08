## Purpose

Restore globally connected subscription agents after a runtime failure while containing resource use and preserving the current session during policy activation.

## ADDED Requirements

### Requirement: Bounded automatic runtime recovery

The connected background runtime SHALL automatically attempt recovery after an unexpected failure, with at most three restart attempts one minute apart. Every attempt MUST retain the existing 2048 MiB job limit and private failure evidence. Persistent failure MUST stop retrying and remain observable. Explicit disconnect MUST leave the integration stopped.

#### Scenario: One runtime crash
- **WHEN** the connected managed runtime fails and its replacement can start
- **THEN** the background host restarts the runtime automatically and republishes the Grok role only after the replacement proves readiness

#### Scenario: Repeated failure
- **WHEN** every automatic restart also fails
- **THEN** the configured retry budget is exhausted without an infinite restart loop and retained logs identify the failed attempts

#### Scenario: Intentional disconnection
- **WHEN** the user disconnects the subscription integration
- **THEN** its owned task and connections are removed and automatic recovery does not reconnect them

### Requirement: Non-disruptive recovery policy activation

The installer SHALL support applying the restart policy to an already installed owned task without stopping, deleting or manually starting its running process. It MUST preserve unrelated task settings, configuration, authentication and links. Ownership conflicts and unfinished lifecycle operations MUST prevent unsafe writes. Preview MUST be read-only; interrupted policy updates MUST be recoverable without stopping the runtime.

#### Scenario: Apply while a session uses the proxy
- **WHEN** the restart policy is applied to a running owned installation
- **THEN** the same proxy process remains ready and subsequent installation checks accept the new task definition

#### Scenario: Interrupted or conflicting activation
- **WHEN** a policy update is interrupted or another writer changes the task or ownership state
- **THEN** recovery restores the recorded state or reports a conflict while preserving foreign changes and recovery evidence

### Requirement: Reusable and verified global recovery

The policy SHALL be maintained in reusable kit sources and applied by new installation and update. Acceptance MUST exercise automatic failure recovery in owned isolated state and verify live policy settings and exact Grok delegation through a fresh global Codex session outside this checkout. It MUST NOT deliberately crash or stop the global proxy used by active sessions.

#### Scenario: Global delivery
- **WHEN** this change is declared complete
- **THEN** the installed task has the bounded policy, the global service is ready, and an externally launched parent receives a tool-derived result from its exact Grok middle child
