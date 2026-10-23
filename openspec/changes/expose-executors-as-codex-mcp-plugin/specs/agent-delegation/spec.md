# Spec Delta

## MODIFIED Requirements

### Requirement: Bounded collaboration and verification

Each workstream SHALL receive a concise objective, sufficient inputs, dependencies, ownership of files and mutable runtime resources, constraints, acceptance checks, an integration consumer, a completion or return condition, and the synchronized base revision of its checkout. Independent edits SHALL use disjoint scopes and, when checkout isolation is needed, the common harness pool defined by `isolated-worktree-workflow`. Executor dispatch SHALL acquire through that pool before launching the session; a task-created ordinary or Codex-managed worktree SHALL NOT substitute for the recorded pool binding. An executor SHALL verify that its checkout HEAD equals the base named in its brief before substantive edits and SHALL stop and report a mismatch instead of repairing synchronization, copying files from another checkout or creating a substitute worktree; changed tracked inputs reach an assignment through a new committed base and a redispatch. Shared desktop sessions, services, installed directories and devices SHALL have one interaction owner, isolated allocation or serialized use; a separate checkout SHALL NOT imply runtime isolation. Executors SHALL report changed files or other concrete results, relevant decisions, checks, validity conditions, restoration state and unresolved issues concisely, with details available on demand under the owning retention policy. The active lead SHALL verify important risks and the combined result without routinely repeating completed investigation or every worker check. A supporting result SHALL count as delivered only after its intended consumer uses it and applicable integrated acceptance passes. The task-wide concurrency limit SHALL come from the orchestration configuration; leadership changes SHALL NOT multiply that limit. Workers SHALL NOT create unsolicited recursive agent trees. Executor sessions SHALL run as single-agent workers: dispatch, continuation and instruction-refresh succession SHALL disable Codex's built-in descendant-agent capabilities, so native agent-spawning tools and unrelated multi-agent instructions are absent whatever the selected model catalog advertises. Authenticated MCP communication with the originating lead SHALL remain available under `executor-mcp-plugin`; disabling native agent capabilities SHALL NOT disable that required channel. The installed launcher SHALL apply the same switch to any Codex process started in an executor-marked environment, including a raw nested `codex` invocation. The kit's executor dispatch interfaces SHALL refuse to run inside an executor session with a concrete error that names the lead as the owner of further delegation. An executor that needs another agent or executor SHALL report that need to the lead instead of creating one. Configured controls SHALL be distinguished from advisory time/token budgets; unsupported hard reasoning-token caps SHALL NOT be claimed.

#### Scenario: Parallel work is integrated
- **WHEN** two executors handle independent workstreams
- **THEN** execution overlaps without conflicting resource ownership, evidence is returned, and the active lead verifies the consumed combined outcome

#### Scenario: An executor starts from the named base
- **WHEN** an executor receives a brief that names the synchronized base revision of its pool slot
- **THEN** it verifies its checkout HEAD equals that revision before substantive edits, and on mismatch returns the exact observed and expected revisions to the lead instead of editing

#### Scenario: A worker discovers an out-of-scope issue
- **WHEN** an executor observes unrelated diagnostics or a change outside its assignment
- **THEN** it preserves unrelated work and reports material information without expanding scope or creating additional agents

#### Scenario: Two workstreams need the same interactive application
- **WHEN** separate agents would otherwise manipulate the same desktop or application session
- **THEN** one agent owns its interaction, conflicting operations are serialized, and independent work uses separate resources within existing aggregate limits

#### Scenario: An isolated investigation needs a parent dependency
- **WHEN** an executor finishes its investigation but cannot exercise integration because a named prerequisite is unavailable
- **THEN** it returns the verified result, unmet prerequisite and validity conditions, and the lead preserves pending integration without restarting the investigation or claiming full completion

#### Scenario: An executor has no agent tools
- **WHEN** an executor session starts under a profile whose model catalog advertises multi-agent support
- **THEN** its provider request advertises no native agent-spawning or native cross-agent messaging tools, authenticated MCP communication with its own lead remains usable, unrelated multi-agent usage guidance is absent, and attempted descendant dispatch is refused with the lead named as its owner

#### Scenario: One isolation contract across task types
- **WHEN** a workstream changes from a main-session experiment to an executor assignment
- **THEN** its checkout remains governed by the same pool capacity, ownership and preservation rules, and any session handoff is explicit rather than an additional worktree allocation

### Requirement: Global lifecycle and recoverability

Sources and setup SHALL activate through the existing linked global lifecycle for new Codex sessions in other projects. Available external models SHALL follow their subscription profile connections; direct native GPT assignment SHALL remain usable when external integration is disconnected. Dispatch SHALL NOT require redundant supplied agent presets. Existing credentials, unrelated user agents and skills, work, MCP integrations and active control channels SHALL be preserved. Updates and disconnection SHALL be verifiable in owned state without stopping a proxy serving other sessions.

#### Scenario: A new external session starts
- **WHEN** the kit is installed and ordinary Codex starts outside this repository
- **THEN** the delegation policy, the `team-lead` skill, orchestration role configuration, board availability, explicit model/effort selection, distinct native conversations inspectable through /agents in the same CLI and task recovery are usable without copying sources or repeatedly performing manual setup

#### Scenario: Subscription integration is disconnected
- **WHEN** its lifecycle is exercised in an isolated installation
- **THEN** removed routes are unavailable for new assignments, native GPT remains usable, recoverable task state is retained, and unrelated services and credentials are preserved

### Requirement: Profile-fixed executor routing never blocks delegation

Kit executor dispatch SHALL route every assignment through the executor
profiles configured in `orchestration.toml`, and a profile's configured model
and reasoning effort SHALL be that assignment's complete explicit selection.
The absence of per-assignment model or effort arguments on executor dispatch
SHALL NOT be treated as a conflict with any selection rule, capability ceiling
or session instruction; explicit per-task model and effort selection applies
to ordinary in-session agents, not to kit executor dispatch. The selected allowed profile, or the configured executor default when omitted,
SHALL be dispatched exactly as configured with no substitute model, provider,
effort, billing route or profile, and a single configured executor profile SHALL be
treated as normal full delegation capacity, not as reduced capability that
justifies keeping executor-suitable work with the lead. Instruction or
preference wording that appears to restrict delegation (model preferences,
effort rules, routing guidance, unknown quota) SHALL bound only which
configured session runs the work and SHALL NOT justify withholding a
dispatch; the discrepancy SHALL be reported while dispatch proceeds. Only a
concrete dispatch failure reported by the dispatch interface or installation check
SHALL block dispatch, and it SHALL be reported with its exact cause and
remedy instead of silent solo continuation.

#### Scenario: Profile dispatch satisfies explicit selection
- **WHEN** an instruction requires explicit model and reasoning effort per assignment and a lead dispatches a kit executor with its configured profile
- **THEN** the profile's configured model and effort are the assignment's explicit selection, the dispatch proceeds without per-assignment model or effort arguments, and no rule conflict is reported

#### Scenario: A single executor profile is full capacity
- **WHEN** `orchestration.toml` configures exactly one executor profile and executor-suitable work exists
- **THEN** the lead dispatches that work to the configured profile and does not treat the single profile, its model or its effort as a reason to keep the work solo or to wait for another route

#### Scenario: Conflicting routing wording is reported, not obeyed
- **WHEN** other instruction text appears to prescribe a different executor model, per-task model/effort arguments or a capability ceiling for delegation
- **THEN** the lead dispatches the configured executor profile anyway and reports the wording discrepancy, instead of classifying executors as blocked and continuing solo

#### Scenario: Only a verified dispatch failure blocks
- **WHEN** executor dispatch is withheld
- **THEN** the blocking cause is a concrete dispatch- or installation-check-reported failure (for example a missing profile, a stale build, an occupied pool or a fetch failure) with its exact cause and remedy reported, and unverified quota, routing or rule-conflict interpretations are not blockers

### Requirement: Stop observed executors on runtime-proven sustained cache loss

Observed managed executor runs SHALL stop automatically when a previously warmed cache suffers three consecutive large misses, regardless of the configured provider or model. Warmup SHALL require a recorded response with at least 100,000 input tokens and at least 90 percent cached input, and SHALL be the runtime proof that this policy applies to that run. The mere presence of a cached-input field, provider branding, or a static model allowlist SHALL NOT establish support. Each qualifying miss SHALL have at least 100,000 uncached input tokens and at least 50 percent uncached input. A valid intervening response below either miss threshold SHALL reset the consecutive count. Cold starts, never-warmed runs, and duplicate usage records SHALL NOT trigger a stop.

The policy SHALL cover the MCP dispatch and observed exact-session continuation routes for every configured executor profile. It SHALL use recorded per-response input and cached-input counters for the exact native session, without decoding reasoning or sending model requests for monitoring. Historical usage on explicit resume MAY establish warmup but SHALL NOT count as new misses. Missing or invalid counters SHALL NOT be invented; unavailable, invalid, or never-proven coverage SHALL be reported with the run's actual resolved provider and model. Monitoring SHALL consume appended data in bounded reads triggered by filesystem/native events and SHALL cover delayed filesystem notifications with a one-second open-file size reconciliation, without periodically rereading unchanged rollout contents. At detection the controller SHALL urgently interrupt the verified native thread and contain that run's owned child work before potentially blocking acknowledgments or receipt writes, without waiting for model work, tests or builds to finish. It SHALL preserve the shared runtime, lead and other executors; terminating a shared app-server SHALL NOT be an acceptable stop mechanism. Usage only becomes available after a response; this policy SHALL NOT claim a hard spending cap or zero further in-flight requests.

#### Scenario: Filesystem notifications are delayed
- **WHEN** the native writer has appended readable usage but Windows has not delivered a file-change notification
- **THEN** the open-file size reconciliation detects the growth and consumes the appended records while the writer remains open
- **AND** unchanged content is not reread by the reconciliation

#### Scenario: A non-DeepSeek provider proves cache warmup
- **WHEN** an executor on any configured provider records a response meeting the warmup thresholds and then three consecutive distinct responses cross both miss thresholds
- **THEN** the executor stops its exact thread and owned child work and reports the provider/model, input, cached-input, and miss evidence with the same partial-work guarantees as any other supported run

#### Scenario: Warmed cache repeatedly fails
- **WHEN** three consecutive distinct responses after warmup cross both miss thresholds
- **THEN** the executor stops its exact thread and owned child work, reports the input/cache-miss evidence and retains its session, checkout, slot binding and partial work
- **AND** the receipt and MCP observation result report a non-success outcome and an explicit continuation remedy, without a fallback model or automatic retry

#### Scenario: Cold start or temporary miss
- **WHEN** the cache has not warmed, a below-threshold response interrupts a sequence of fewer than three qualifying misses, or numeric counters never establish a warmed response
- **THEN** execution continues and the policy does not treat cold input, an isolated loss, or unproven support as a sustained regression

#### Scenario: Resume and identity isolation
- **WHEN** an exact session is resumed and its earlier history includes a cache-loss sequence, or another session's usage is present
- **THEN** historical losses and foreign-session counters do not cause a stop, while new qualifying losses after historical warmup do

#### Scenario: Cleanup is incomplete
- **WHEN** the policy triggers but interruption and containment cannot verify that the target's owned work has ended
- **THEN** the run reports a partial stop with its cleanup cause instead of claiming that spending has stopped

#### Scenario: No usable usage evidence
- **WHEN** the exact session's per-response usage is unavailable or invalid
- **THEN** the surface and receipt disclose missing coverage, without fabricating cache performance or claiming an enforced spending cap

### Requirement: Cache-guard diagnostics use the native warning surface

The managed executor host SHALL distinguish routine cache-guard status, degraded cache-guard coverage, and a terminal cache-loss stop. Routine status transitions SHALL be recorded in the run receipt and host log without becoming user-facing warnings. Degraded coverage diagnostics SHALL be delivered through Codex's native warning presentation when a native frontend is attached, so they participate in the frontend's warning count and viewer rather than being written directly into the terminal input area. The host SHALL record whether each degraded diagnostic was delivered natively, could not be delivered, or had no native warning consumer. A delivery failure SHALL NOT be reported as a shown warning, and a compatibility surface without a native warning consumer SHALL use only its existing receipt/log surface.

A terminal cache-loss stop SHALL retain its non-success outcome, stop cause, bounded evidence, and explicit MCP recovery action; it SHALL NOT be represented as a dismissible warning that replaces the existing result.

#### Scenario: Degraded coverage is visible in the native warning viewer
- **WHEN** exact-session usage becomes invalid or unavailable while a native executor frontend is attached
- **THEN** the diagnostic is available through the native warning count and viewer for that frontend
- **AND** the host writes no cache-guard diagnostic line directly into the terminal input area

#### Scenario: Routine status is not presented as a warning
- **WHEN** monitoring starts, first observes usable per-response usage, warms, or remains below loss thresholds
- **THEN** the receipt or host log carries the status without adding a native user warning or terminal diagnostic line

#### Scenario: Native warning delivery fails
- **WHEN** a degraded diagnostic cannot be delivered to the attached native frontend
- **THEN** the run records undelivered warning state and the underlying receipt/log evidence without claiming user visibility
- **AND** the failure does not by itself terminate otherwise healthy model work

#### Scenario: A compatibility surface has no native warning consumer
- **WHEN** an observed compatibility renderer is used instead of a native frontend
- **THEN** degraded coverage remains available through the receipt and host log, and the run does not claim native warning presentation

#### Scenario: Cache loss terminates the run
- **WHEN** runtime-proven sustained cache loss triggers the stop policy
- **THEN** the native surface, receipt, and MCP observation result report the stop cause and recovery path as a non-success terminal outcome
- **AND** that outcome is not reduced to a warning-list entry without result state

### Requirement: Recover cache-stopped work in a fresh conversation without losing progress

The cache-stop receipt and MCP observation SHALL give the lead an actionable fresh-conversation recovery operation for the same verified slot, owner and predecessor session. MCP fresh recovery SHALL preserve the original worktree, commits, uncommitted and untracked files and saved verification evidence without resetting, cleaning or releasing the slot. It SHALL refuse a stale predecessor identity or a live owner. It SHALL start a new conversation with the configured model binding, original assignment and bounded visible progress, retain the predecessor rollout for targeted lookup, and require verification of interrupted commands before reusing results. Missing original assignment SHALL require explicit assignment input rather than inventing a new task. Recovery SHALL NOT resume the old full context or automatically loop after repeated cache failures.

#### Scenario: Stop during a build and continue the original task
- **WHEN** a cache-stopped executor leaves completed edits and an interrupted build, and the lead requests restart for the recorded predecessor
- **THEN** the fresh conversation uses the same worktree and original assignment, receives the bounded visible checkpoint, and checks the saved work and build outcome before continuing
- **AND** the old session remains available and the interrupted build is not reported as successful

#### Scenario: Stale recovery command
- **WHEN** a restart names a predecessor that no longer occupies the slot or its owner is still live
- **THEN** it refuses before model dispatch or worktree mutation instead of replacing the current occupant

### Requirement: Concise executor clarification instructions

Every newly spawned executor SHALL receive concise installed guidance for the installed MCP communication tool in both free-text and structured assignment paths. Supported continuation paths SHALL retain current guidance without duplicating whole manuals. The guidance SHALL explain that the tool automatically addresses the originating lead, questions request replies by default, waiting keeps the session and worktree available without a manual keep-alive loop, independent authorized work can continue, and a notification mode is available when no reply is needed. Executors SHALL use this channel exceptionally for a material ambiguity, authority/access boundary or concrete dependency they cannot resolve after inspecting available facts. They SHALL NOT use it for routine progress, repeated status messages or ordinary implementation errors they can correct. Durable task state and decisions SHALL remain on the existing bd board. The mechanism SHALL NOT enable native agent-spawning tools, recursive delegation or arbitrary agent messaging.

The combined required owned guidance for the exchange SHALL NOT grow when superseded escalation/addressing instructions are removed. Accounting SHALL include generated briefs, skill bodies and required references/help for the same assignment inputs. Instructions SHALL explain the action-required observation result without teaching lifecycle bookkeeping or requiring another polling loop.

#### Scenario: Free-text assignment asks a legitimate question
- **WHEN** an executor started with a free-text assignment encounters an unresolved material acceptance ambiguity
- **THEN** its injected guidance identifies the MCP question operation and explains how to await an answer while preserving the bd assignment and independent authorized work

#### Scenario: Structured assignment needs missing authority
- **WHEN** an executor started with a structured assignment reaches an authority boundary it cannot resolve
- **THEN** it knows to contact its own lead through MCP, records the blocker on bd, and keeps the decision-dependent action pending

#### Scenario: An ordinary implementation error occurs
- **WHEN** the executor encounters a syntax error or API mismatch that its available tools can investigate
- **THEN** the guidance directs it to diagnose and correct the issue within its scope rather than sending a message solely because an error occurred

#### Scenario: Lead receives a clarification request
- **WHEN** the lead receives a message envelope from one of its spawned executors
- **THEN** its installed guidance explains the injected sender/worktree metadata and direct MCP reply, preserves the capable executor's ownership and directs durable decisions back to bd

#### Scenario: Waiting needs no instruction ritual
- **WHEN** a request remains unanswered after the executor has no independent work
- **THEN** the executor can end the current turn and rely on the native-backed lifecycle rather than running repeated sleeps, status questions, receipt edits or additional model turns to stay available

### Requirement: Concise guidance relies on executable mechanics

Owned delegation guidance SHALL state when to delegate, the assignment and acceptance contract, the supported MCP tools and recovery decisions. Deterministic path, routing, lifecycle, ownership, delivery and cleanup checks already guaranteed by those tools SHALL NOT require the agent to repeat their implementation as a manual checklist. Shortening SHALL preserve model selection, visibility, board ownership, escalation, partial-work protection and complete acceptance.

The combined affected owned instructions SHALL decrease in both UTF-8 bytes and whitespace-delimited words from an identified baseline. For the same representative ordinary-task, lead-dispatch, executor-reply and recovery scenarios, the required instruction load SHALL NOT grow. Accounting SHALL include global/developer instructions, skill metadata and bodies, required references, generated briefs and mandatory tool guidance; moving text among those surfaces SHALL NOT hide growth. User assignment payloads SHALL be held constant. Protected external instructions SHALL remain unchanged and separately identified. Less text SHALL be accompanied by successful use of the actual installed tools, not a wording-only test.

#### Scenario: An executor needs clarification
- **WHEN** a spawned executor uses the shortened instructions to ask its lead and consume a reply
- **THEN** it uses the supported MCP operations and automatic lifecycle, without reconstructing addresses, polling the model or losing its work, and the lead still applies the acceptance rules

#### Scenario: A checklist is moved to a mandatory reference
- **WHEN** an instruction revision shortens a skill by adding a required reference or expanding its generated brief
- **THEN** that content remains in the measured instruction load and cannot be counted as a reduction merely because the skill file became shorter

### Requirement: Visibility policy does not capture conversation pixels

Visibility of each active model conversation SHALL remain a native UI or controller obligation established before its first model request. For MCP executors, distinct titled conversations accessible through the same CLI's native /agents interface SHALL satisfy this obligation without separate terminal surfaces or simultaneous display. Until required native access is available, agents SHALL keep acceptance-independent work in the already visible main conversation and MUST NOT start hidden model processes. They MUST NOT satisfy visibility by screenshots of Codex or other agent windows, sending those images to a model, polling window pixels, or resizing, moving or arranging desktop windows. Ordinary conversation navigation SHALL NOT count as lost visibility. Lost controlling CLI access SHALL be reported and restored by its owning runtime path, not treated as a desktop visual task.

#### Scenario: A child conversation needs a separate view
- **WHEN** a lead would dispatch an executor and its distinct conversation cannot be made accessible through the controlling CLI's native interface
- **THEN** it reports the missing capability before model work and preserves the existing work without hidden dispatch, screenshots or desktop rearrangement

#### Scenario: Window identity is needed for an owned desktop target
- **WHEN** an authorized desktop task needs to select a non-Codex window
- **THEN** list/title/state identify the window, and a screenshot is used only if a visual question remains

## REMOVED Requirements

### Requirement: Interrupted pooled executors resume through the pooled command

**Reason**: The separate command/terminal resume contract is replaced by native-thread continuation through MCP.

**Migration**: `executor-mcp-plugin` / Preserve exact continuation and fresh recovery; existing lease reconciliation and partial-work invariants remain required.

### Requirement: Addressed executor message and stop commands

**Reason**: Active executor control moves to MCP and native conversation input instead of CLI text sources and spill-pointer delivery.

**Migration**: `executor-mcp-plugin` / Intent-based executor interface, Verified caller and run authority, Native bidirectional communication, Literal input and large native records, and Urgent stop isolates one executor.

### Requirement: Globally usable native-backed lead exchanges

**Reason**: Installed conversation acceptance belongs to the single plugin's lifecycle contract.

**Migration**: `executor-mcp-plugin` / Installed plugin lifecycle and Correlated questions remain actionable; actual model-visible question/reply and continued work remain acceptance requirements.

### Requirement: Minimal delegation command forms in guidance

**Reason**: Migrated CLI forms and their fallback examples must disappear from active context.

**Migration**: `executor-mcp-plugin` / MCP-only active executor guidance; retained Concise guidance relies on executable mechanics owns instruction-load acceptance.

