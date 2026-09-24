## Purpose

Keep the Windows desktop usable during concurrent local agent work by automatically sharing one CPU budget across sessions, projects and agent tools, while retaining explicit uncapped operation and truthful coverage diagnostics.

## ADDED Requirements

### Requirement: One default CPU allowance SHALL cover concurrent agent work

On a Windows host, agent sessions and their local tools running for the same Windows account SHALL share one default CPU ceiling of 75% of total host CPU capacity. The allowance SHALL NOT multiply with the number of projects, working trees, agent homes, model providers, launchers, sessions, commands or threads. Normal launches SHALL require no per-command wrapper or shell-specific instruction to receive this policy. Explicit uncapped invocations and the separately specified visible fail-open fallback are exceptions to enforced admission. Unrelated applications and operating-system processes SHALL remain outside the controlled group; the budget SHALL NOT be represented as a ceiling on total workstation activity.

#### Scenario: Sessions in different projects perform CPU work
- **WHEN** two normal agent sessions from different projects and agent homes execute independent CPU-intensive work concurrently
- **THEN** their sessions and local tools consume the same aggregate 75% allowance rather than receiving separate 75% allowances
- **AND** another unrelated desktop application is not enrolled in that allowance

#### Scenario: A new session starts without a CPU option
- **WHEN** an installed agent entry point starts normally without a CPU override
- **THEN** the session joins the existing default allowance or establishes that same account-wide allowance before useful work starts

### Requirement: Admission SHALL cover ordinary agent and tool launch routes

The delivered installation SHALL cover Codex sessions, interactive and executor/resume entry points, their native command descendants, and kit-owned shared MCP services and backends. Coverage SHALL include background or sibling services that execute agent work without being descendants of the current CLI. Successful capped admission SHALL take effect before newly launched payloads can perform useful work and SHALL be independent of the command language or shell. A timer that notices a process only after it begins executing SHALL NOT satisfy this admission guarantee. Admission failures SHALL follow the visible fail-open requirement instead of being reported as capped starts.

Installed route and process identity SHALL determine coverage; executable names alone SHALL NOT authorize enrolling or changing an unrelated process. Full coverage SHALL NOT be claimed while a supported active agent route or shared tool remains outside the allowance without an explicit exception. Launching one agent from a shared terminal SHALL NOT enroll the entire terminal and its unrelated tabs. Remote compute and arbitrary unrelated applications are outside this local-account capability.

#### Scenario: A tool launches an executable directly
- **WHEN** an agent tool launches a native CPU-intensive executable without PowerShell and that executable starts grandchildren
- **THEN** the executable and its grandchildren are covered from the start of their execution

#### Scenario: An executor starts in a new terminal tab
- **WHEN** an executor or resumed session starts through an installed dispatch route in a new terminal tab
- **THEN** it shares the same CPU allowance as other sessions without limiting unrelated terminal tabs

#### Scenario: A shared backend already serves another client
- **WHEN** a session sends work to a kit-owned backend that is shared with another session
- **THEN** backend work participates in the aggregate allowance independently of which client started it

#### Scenario: A supported agent connection is incomplete
- **WHEN** installation or inspection finds a supported agent entry point or owned service that is not admitted
- **THEN** diagnostics identify that route and the corrective action and report coverage as incomplete
- **AND** a list of registered agents alone does not count as successful coverage

### Requirement: Resource policies SHALL compose without accidental CPU multiplication

Interactive sessions SHALL remain concurrently usable and SHALL NOT hold the exclusive heavy-command queue for their entire lifetimes. Existing batch admission, memory, cancellation, deadline and cleanup contracts SHALL remain in force for operations that use them. An inherited aggregate CPU allowance SHALL be applied once; a nested invocation SHALL verify actual membership rather than trust a copied environment marker. If an operation has an explicit lower CPU ceiling, the effective limit SHALL preserve its documented meaning relative to host CPU capacity and SHALL be reported. Default inner limits SHALL NOT accidentally turn the global allowance into a product of nested percentages.

#### Scenario: A managed build runs inside a capped session
- **WHEN** a capped session invokes the existing heavy-command entry point and a nested managed build
- **THEN** the build remains inside the one aggregate allowance without a duplicate default CPU cap or recursive admission deadlock
- **AND** the heavy-command memory, cancellation, timeout and cleanup behavior remains intact

#### Scenario: A command has an intentionally lower limit
- **WHEN** a command requests a CPU ceiling lower than the shared ceiling
- **THEN** its effective host-relative ceiling is preserved and reported while the whole group still obeys the shared budget

### Requirement: Shared accounting SHALL preserve independent process lifecycles

The CPU accounting group SHALL NOT transfer one session's termination authority over peer sessions or shared services. Normal exit, abrupt owner death, cancellation and background continuation SHALL retain the established lifecycle of each process owner. A session ending SHALL NOT reset another session's allowance or kill its work. Reconnection to existing budget state SHALL validate account ownership and live process identity and SHALL NOT adopt an unrelated object solely because its name matches. Loss or restart of the budget coordinator, if one is used, SHALL NOT silently remove enforcement from already admitted work.

#### Scenario: One of two sessions exits or crashes
- **WHEN** one session exits normally or its owner crashes while a peer and a shared backend remain active
- **THEN** cleanup follows the exiting session's ownership contract, the peer and backend retain their independent lifecycles, and the shared CPU allowance remains enforced

#### Scenario: A coordinator reconnects to existing state
- **WHEN** budget management restarts while previously admitted work is still running
- **THEN** enforcement is retained and reconnect validates the existing ownership and identities before admitting new work
- **AND** it does not create a second independent default allowance

### Requirement: Uncapped operation SHALL be explicit and scoped

The user SHALL be able to request an uncapped session or command through a documented installed entry point. This exception SHALL be visible before payload execution and SHALL apply only to the requested invocation and its declared descendants. It SHALL NOT disable or raise the allowance for other sessions, persist as an implicit default for later invocations, or exempt shared services that also serve capped clients. Exception diagnostics SHALL make clear that combined host agent load can exceed 75% while the exception runs. A flag that leaves the requested payload subject to an inherited aggregate cap SHALL NOT be reported as successful uncapped operation.

#### Scenario: One command needs an uncapped measurement
- **WHEN** the user explicitly launches one command without the CPU cap while normal sessions remain active
- **THEN** the command actually runs outside that CPU allowance, its mode is clearly reported, and the normal sessions retain their original shared cap
- **AND** the next invocation without that exception is capped by default

#### Scenario: An uncapped session uses an existing shared service
- **WHEN** an uncapped session accesses a shared service that also serves capped sessions
- **THEN** the service remains in the shared group and the exception status does not claim that its work became uncapped

### Requirement: Policy and coverage SHALL be observable without protocol contamination

Machine policy, temporary exceptions and runtime identities SHALL remain in the existing machine-local ownership lifecycle, outside shared source and consumer repositories. Installed inspection SHALL report the configured budget, effective CPU enforcement, covered entry points and process groups, explicit uncapped exceptions, degraded fallback launches, pending restart boundaries and incomplete coverage. It SHALL distinguish kernel configuration and membership from measured CPU consumption. Human startup diagnostics SHALL be concise; they SHALL NOT corrupt native stdout, MCP STDIO or other machine protocols. Querying policy or coverage SHALL NOT start model work, run a load test or change the policy.

#### Scenario: The user checks the current policy
- **WHEN** the user invokes the installed CPU-budget status operation
- **THEN** it identifies the common 75% allowance, its actual enforcement and coverage, any uncapped exceptions and any action needed for full coverage
- **AND** an unknown or inaccessible member is reported explicitly rather than counted as covered

#### Scenario: A machine protocol process starts
- **WHEN** a covered MCP process starts under the policy
- **THEN** its protocol stream remains valid and CPU-budget diagnostics use the established diagnostic channel

### Requirement: CPU admission failures SHALL preserve launch availability with a warning

If the default CPU policy cannot be established or verified, the installed entry point SHALL continue to launch the requested agent or tool without a guarantee of that cap and SHALL emit a visible warning before useful payload execution. The warning SHALL identify the failed limit, concrete cause, affected launch scope and corrective action. It SHALL distinguish missing or unknown enforcement from a deliberately uncapped invocation. Coverage status SHALL remain degraded while such work is outside verified admission; other admitted sessions SHALL retain their budget. Recovery SHALL NOT change the persistent default, silently report success, enter a retry loop or start a second payload after the first has begun execution. A missing or invalid requested executable SHALL remain an ordinary launch error and SHALL NOT trigger an arbitrary substitute.

#### Scenario: The operating system refuses the CPU policy
- **WHEN** CPU setup or membership verification fails before the requested payload starts
- **THEN** the same requested payload starts once with its arguments, cwd, streams and exit behavior preserved, and a warning states that the shared cap is not guaranteed
- **AND** other sessions remain capped and the next normal invocation still attempts the default policy

#### Scenario: Shared harness inputs are unavailable
- **WHEN** optional harness source, registration or shared features are unavailable but the original agent executable remains usable
- **THEN** the installed bootstrap retains upstream launch availability, establishes the shared cap if possible, and otherwise uses the warned degraded fallback

#### Scenario: A failure is detected after payload execution began
- **WHEN** monitoring loses confidence in enforcement after the payload has started
- **THEN** the existing invocation is reported as degraded without replaying, restarting or duplicating its work

### Requirement: Activation and rollback SHALL preserve ongoing work

Installation and update SHALL connect the policy globally through the existing recoverable lifecycle and verify it from outside the kit checkout. They SHALL preserve unrelated configuration, user overrides and upstream update behavior. Already running sessions and services SHALL either be enrolled with verified membership and compatible lifecycle semantics or be reported as requiring a safe restart. Installation SHALL NOT terminate ongoing work to manufacture coverage. Until outstanding ordinary processes have safely transitioned, inspection SHALL report incomplete activation. Rollback SHALL preserve active work and later user edits and explicitly report when the default coverage is no longer provided.

#### Scenario: The policy is installed while sessions are active
- **WHEN** activation encounters sessions or shared services started by an older installation
- **THEN** it preserves their work and reports verified enrollment or the exact restart boundary
- **AND** it claims complete activation only after the uncovered work has safely transitioned or ended

#### Scenario: The user rolls back the change
- **WHEN** a user requests rollback while other work is active
- **THEN** installation ownership and running work are preserved, the resulting coverage is reported, and unrelated settings are not overwritten

### Requirement: Acceptance SHALL demonstrate actual aggregate enforcement

Acceptance SHALL exercise installed agent and tool entry points with owned controller-free workloads from outside this checkout. It SHALL cover concurrent projects and agent homes, Codex routes, direct native descendants, shared backends, nested batch work, explicit exceptions, and exit/recovery behavior. Measurements SHALL report the workload, duration, actual host logical-processor count, aggregate CPU-time delta and kernel membership/settings for the same exercised scope. Measurement bounds SHALL account for sampling and scheduler granularity and SHALL be declared before evaluating results. Sustained unexplained consumption above the requested ceiling SHALL block acceptance; successful configuration readback alone SHALL NOT establish enforcement. Acceptance SHALL also exercise an ordinary interactive action under contention and record its responsiveness and practical limits.

#### Scenario: Multiple workloads saturate the admitted group
- **WHEN** repeatable parallel workloads saturate normal agent work for a declared measurement interval
- **THEN** the measured aggregate consumption stays within the requested ceiling and the declared measurement bound, and all exercised routes have verified membership
- **AND** a short fixture alone does not replace verification of a representative longer build or test workload

#### Scenario: Readback and observed consumption disagree
- **WHEN** the operating system reports the requested CPU setting but the measured workload persistently exceeds the acceptance bound
- **THEN** the result remains a failed or unresolved enforcement check until the discrepancy is explained and the required behavior is demonstrated
- **AND** increasing the tolerance merely to pass the same observation does not satisfy acceptance
