## ADDED Requirements

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
