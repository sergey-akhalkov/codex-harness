## Purpose

Keep an authorized development task progressing across the available GPT, Z.AI and Grok subscriptions, preserving accepted requirements and completed work when an executor or the normal leader cannot continue.

## ADDED Requirements

### Requirement: Task control survives leader unavailability

The installed task runtime SHALL observe assignment outcomes and perform already authorized recovery without requiring a successful response from the unavailable leader. It SHALL preserve the ordinary supported Codex interaction and expose the active leader, material recovery transitions and final result. Unsupported control contracts SHALL produce an explicit capability limitation before activation, never a silent claim of automatic recovery. Active-task waiting and routine event handling SHALL require no model calls. A runtime that remains active SHALL continue eligible work until completion, explicit stop or an observable dependency prevents progress, subject to the simultaneous conversation visibility requirement.

#### Scenario: GPT cannot issue a fallback instruction
- **WHEN** an active GPT leader receives a confirmed quota-exhaustion response before it can request recovery
- **THEN** the runtime initiates the agreed leadership transfer without another GPT response or user re-prompt and preserves active executor work

#### Scenario: Installed runtime cannot support task control
- **WHEN** required session control or event delivery fails its compatibility check
- **THEN** activation reports the missing capability and preserves the previous usable installation instead of enabling partial recovery

### Requirement: Simultaneous visibility of all active conversations

Every active leader, executor and model-backed auxiliary conversation SHALL have its own simultaneously visible window or pane, established automatically before its first model request. The user SHALL see each assignment, role, actual provider/model, supported effective reasoning effort, live messages and tool activity, and running/waiting/error/completed state. Available usage SHALL be attributable to its conversation; missing usage SHALL remain unknown. A switchable list, hidden process, raw log or single chat identity masking several conversations SHALL NOT satisfy this requirement. Automatic helpers SHALL be made visible and attributable or disabled. This requirement concerns available conversation events, not disclosure of opaque internal reasoning. Completed and replaced conversations SHALL remain inspectable under the private retention policy.

#### Scenario: Leader and two executors run together
- **WHEN** the normal lead dispatches independent Z.AI and Grok work
- **THEN** all three conversations are simultaneously visible with distinct identities and live activity, without manually locating processes or switching between hidden chats

#### Scenario: Leadership changes provider
- **WHEN** GPT quota exhaustion requires a fresh Z.AI lead
- **THEN** the successor has its own identified visible conversation before making a model request, the handoff is explicit, and the previous lead history and active worker panes remain available

#### Scenario: A model-backed helper would run
- **WHEN** a title, search, vision or other auxiliary operation would call a model
- **THEN** its activity and provider are visible and attributable before dispatch, or that automatic call is disabled

#### Scenario: Conversation visibility is lost
- **WHEN** a required conversation view fails or closes and no other attached view displays that active conversation
- **THEN** the runtime suspends new model dispatch, reconciles in-flight effects without replay, reports the view failure, and restores simultaneous visibility before continuing the still-authorized task

### Requirement: Durable authorized handoff

A recoverable task SHALL retain its workspace identity, accepted objective, constraints and authorization, current decisions, active leader, assignments and owners, visible partial results, acceptance state and next useful action. State SHALL be saved before dispatch and ownership transitions and reconciled after interruption. Provider credentials, raw private evidence and task state SHALL remain in host-private storage. Reassignment SHALL NOT expand authorization, change a read-only/exploration task into implementation, drop unfinished requirements or transfer opaque reasoning state between providers.

#### Scenario: Quota fails after a partial edit
- **WHEN** an executor has changed owned files but cannot complete the next request
- **THEN** recovery retains the changes and check status and gives the replacement enough visible context to continue without recreating completed work

#### Scenario: Exploration leadership changes
- **WHEN** a read-only planning task transfers from GPT to Z.AI
- **THEN** the replacement inherits the planning boundary and cannot treat the transfer as permission to implement

### Requirement: Single ownership during recovery

Each active assignment and the task's leadership SHALL have one authoritative owner. Before replacement can mutate an assignment's resources, the runtime SHALL establish that the previous owner is stopped, relinquished or otherwise unable to continue those mutations. Delayed results SHALL be reconciled against the assignment attempt and current ownership before acceptance. Completed checks and effects SHALL be reused only when applicable to current inputs; ambiguous external effects SHALL be inspected rather than blindly replayed. Unrelated files, processes and tasks SHALL remain untouched.

#### Scenario: A delayed previous executor returns
- **WHEN** a previous attempt reports completion after a replacement owns the assignment
- **THEN** the result is retained for reconciliation and cannot overwrite the new owner's work or independently mark the task complete

#### Scenario: An external operation has uncertain completion
- **WHEN** a connection fails after an authorized operation may have taken effect
- **THEN** recovery checks its outcome or retains an explicit blocked dependency instead of repeating an irreversible effect automatically

### Requirement: Cause-aware provider availability

The runtime SHALL distinguish confirmed quota exhaustion, temporary request throttling, authentication/model unavailability, transport failure and invalid/incomplete task output using available provider evidence. A generic rate-limit status SHALL NOT alone establish weekly exhaustion. It SHALL retain failure scope, observation time and reset/retry information when supplied. Confirmed unavailable capacity SHALL not receive repeated new assignments while the same condition persists; a recovered route SHALL return through a bounded eligible check or actual request after relevant conditions change. An unavailable common proxy SHALL not be misclassified as exhausted quotas on every provider. It SHALL preserve the independent bounded proxy-restart policy.

#### Scenario: Short-lived throttling occurs
- **WHEN** a provider reports temporary throttling with usable retry guidance
- **THEN** the runtime paces affected requests or new dispatch within that guidance without declaring the weekly subscription exhausted or racing an active attempt

#### Scenario: Quota is exhausted until a known reset
- **WHEN** a provider reports exhausted capacity with a reset time
- **THEN** eligible work moves to available capable routes, affected capacity is excluded until recovery is eligible, and routine checks consume no model calls

#### Scenario: Only an ambiguous failure is available
- **WHEN** an error does not establish quota scope or remaining capacity
- **THEN** the original cause and uncertainty remain visible, retries are bounded according to the failure, and no invented quota remainder or reset is reported

### Requirement: Capability-aware continuation

Reassignment SHALL preserve the assignment's reasoning, modality, tool access and acceptance needs. For suitable text work, available Z.AI and Grok SHALL be preferred over spending GPT capacity; demanding work beyond Grok's demonstrated suitability SHALL use an available capable route. Image-dependent work SHALL NOT be silently assigned to a text-only route. Lack of one capability SHALL block only dependent work while other accepted work continues. If no suitable route remains, the task SHALL retain its unfinished state and resume eligible work when capacity returns, without purchases or an endless retry loop.

#### Scenario: Z.AI is exhausted
- **WHEN** Z.AI cannot continue and Grok is available
- **THEN** Grok receives suitable unfinished assignments, while assignments beyond its capability use an available GPT route or remain preserved as dependencies

#### Scenario: Grok is exhausted during visual work
- **WHEN** required image input cannot be processed by Grok
- **THEN** an available image-capable GPT route handles that dependency, or it waits while independent text work continues on Z.AI

#### Scenario: All suitable capacity is exhausted
- **WHEN** no connected subscription can serve the next required work
- **THEN** the task retains its checkpoint, reports the reason and any known recovery time, and schedules bounded recovery without repeated model requests

### Requirement: Temporary Z.AI leadership and GPT return

On confirmed GPT quota exhaustion, available Z.AI SHALL take leadership of the already agreed task with current decisions, assignments and acceptance intact. It SHALL coordinate capable executors, consume their results and continue work within the established scope. Problems beyond available capability SHALL remain explicit dependencies rather than being guessed away. GPT SHALL regain normal leadership at a safe decision boundary after verified recovery, without interrupting a healthy executor or duplicating completed work. If both GPT and Z.AI are unavailable, capable already authorized executors SHALL continue their assignments and the runtime SHALL preserve decisions requiring a suitable leader.

#### Scenario: GPT expires while both external executors are active
- **WHEN** the lead loses GPT capacity while Z.AI and Grok have work in progress
- **THEN** the runtime establishes one Z.AI lead without duplicating its existing assignment, preserves Grok's work and the task-wide executor bound, and obtains the next useful decision from Z.AI

#### Scenario: GPT returns during a healthy worker attempt
- **WHEN** GPT capacity has recovered but an executor is still productively working
- **THEN** leadership returns at a safe boundary with current decisions and evidence while the executor retains its assignment

#### Scenario: No suitable lead is currently available
- **WHEN** GPT and Z.AI are unavailable but Grok can finish an existing assignment
- **THEN** Grok can deliver that result, hard unresolved decisions remain pending, and the task does not claim full completion before required acceptance

### Requirement: Restart recovery and explicit stop

After an unexpected controller or client interruption, the runtime SHALL reconcile saved task and process state and restore required visible conversation views before resuming active authorized work. It SHALL reconnect or recover surviving assignments instead of starting duplicate copies. Explicit user stop, task cancellation or integration disconnection SHALL disable automatic dispatch for the affected task and SHALL not be undone by a quota reset or process restart. Stop SHALL preserve recoverable partial work and prevent orphaned owned writers. Restart and resume SHALL retain the original scope and instruction precedence. Losing one client SHALL NOT suspend another task whose required conversations remain visible.

#### Scenario: Controller restarts with surviving workers
- **WHEN** the controller restarts after failure and workers may still be active
- **THEN** ownership and liveness are reconciled before any reassignment and only the still-active authorized task resumes

#### Scenario: User stops a task waiting for quota
- **WHEN** the user explicitly stops a suspended task and a subscription later resets
- **THEN** it remains stopped until the user resumes it, with its partial work available

### Requirement: Globally verified orchestration

Delivery SHALL include installation, update, check, recovery and disconnection through the supported global kit lifecycle. Fresh ordinary Codex sessions outside this checkout SHALL exercise actual Z.AI and Grok tool work and the task controller. Failure acceptance SHALL use owned targets and deterministic quota/transport faults rather than exhausting accounts or disrupting the shared live proxy. It SHALL cover a GPT refusal before fallback, worker refusal after partial work, delayed completion, temporary throttling, modality mismatch, unavailable telemetry, all-capacity exhaustion, leadership return, restart and explicit stop. A real external development task SHALL prove that the results are consumed and required acceptance passes; a toy fixture alone SHALL NOT establish complete delivery.

#### Scenario: Recovery is accepted without exhausting a subscription
- **WHEN** an owned test injects a realistic quota failure at the leader and worker boundaries
- **THEN** the installed control path performs the expected handoff and preserves work, while separate bounded live requests verify the actual subscribed model bindings

#### Scenario: A real consuming task completes
- **WHEN** the globally installed workflow completes a locally selected external development task
- **THEN** its actual result and applicable checks demonstrate integrated use, evidence names the tested identities privately, and public records contain no consumer-specific data
