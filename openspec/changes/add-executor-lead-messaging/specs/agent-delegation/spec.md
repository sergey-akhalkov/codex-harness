## ADDED Requirements

### Requirement: Concise executor clarification instructions

Every newly spawned executor SHALL receive concise installed guidance for `codex-harness lead message` in both free-text and structured assignment paths. Supported continuation paths SHALL retain current guidance without duplicating whole manuals. The guidance SHALL explain that the command automatically addresses the originating lead, questions request replies by default, waiting keeps the session and worktree available without a manual keep-alive loop, independent authorized work can continue, and `--notify` is available when no reply is needed. Executors SHALL use this channel exceptionally for a material ambiguity, authority/access boundary or concrete dependency they cannot resolve after inspecting available facts. They SHALL NOT use it for routine progress, repeated status messages or ordinary implementation errors they can correct. Durable task state and decisions SHALL remain on the existing bd board. The mechanism SHALL NOT enable native agent-spawning tools, recursive delegation or arbitrary agent messaging.

The combined required owned guidance for the exchange SHALL NOT grow when superseded escalation/addressing instructions are removed. Accounting SHALL include generated briefs, skill bodies and required references/help for the same assignment inputs. Instructions SHALL explain the watch action-required result without teaching lifecycle bookkeeping or requiring another polling loop.

#### Scenario: Free-text assignment asks a legitimate question
- **WHEN** an executor started with `--exec` encounters an unresolved material acceptance ambiguity
- **THEN** its injected guidance identifies the minimal lead message command and explains how to await an answer while preserving the bd assignment and independent authorized work

#### Scenario: Structured assignment needs missing authority
- **WHEN** an executor started with `--assignment` reaches an authority boundary it cannot resolve
- **THEN** it knows to contact its own lead through lead message, records the blocker on bd, and keeps the decision-dependent action pending

#### Scenario: An ordinary implementation error occurs
- **WHEN** the executor encounters a syntax error or API mismatch that its available tools can investigate
- **THEN** the guidance directs it to diagnose and correct the issue within its scope rather than sending a message solely because an error occurred

#### Scenario: Lead receives a clarification request
- **WHEN** the lead receives a message envelope from one of its spawned executors
- **THEN** its installed guidance explains the injected sender/worktree metadata and one-command reply, preserves the capable executor's ownership and directs durable decisions back to bd

#### Scenario: Waiting needs no instruction ritual
- **WHEN** a request remains unanswered after the executor has no independent work
- **THEN** the executor can end the current turn and rely on the native-backed lifecycle rather than running repeated sleeps, status questions, receipt edits or additional model turns to stay available

### Requirement: Globally usable native-backed lead exchanges

The kit SHALL deliver lead messaging, reply addressing, waiting behavior, help and instructions through its existing installation/update/recovery lifecycle. Fresh supported lead and executor sessions outside the kit checkout SHALL be able to complete a question, waiting and reply round trip using the installed entry points, preserving the same native sessions, worktrees and actual configured models/providers/efforts. Normal operation SHALL require no private endpoint setup, manually copied IDs or kit checkout access. Installed acceptance SHALL include actual model-visible receipt of the question and reply and continued executor work; a mock-only test, help output or successful local record write SHALL NOT establish that result. Existing explicitly addressed executor message and stop commands SHALL remain usable, and unrelated tools/settings and existing recovery guarantees SHALL be preserved.

#### Scenario: Fresh installed consumer completes the exchange
- **WHEN** an ordinary installed lead spawns an executor in an external consuming project and that executor requests a decision
- **THEN** the lead sees the question with accurate metadata, answers with the supplied reply reference, and the same waiting executor receives the answer and continues its assigned work without resume or another conversation

#### Scenario: Existing session lacks the new binding
- **WHEN** a legacy executor without a verified originating relationship attempts lead messaging
- **THEN** the command reports the missing binding and supported recovery instead of fabricating a lead from local state, and unrelated installed functionality remains usable

## MODIFIED Requirements

### Requirement: Addressed executor message and stop commands

The delegation kit SHALL expose an executor message command and an executor stop command that address one pooled executor run through the accepted checkout, slot, owner and exact recorded session identity. Executor message SHALL additionally accept a verified `--reply-to MESSAGE_ID` that resolves the original sender without requiring those manual address fields. Both commands SHALL verify the addressed run's actual identity and lifecycle before acting, SHALL behave deterministically for completed, stopped, interrupted or unavailable runs by reporting the observed state with a supported next action, and SHALL NOT start a new conversation, reset or clean the slot, or release it automatically. A waiting-for-reply run SHALL remain live and directly addressable, and a correlated reply SHALL continue that same conversation without a separate resume command. Message delivery SHALL preserve the run's conversation, model, provider, reasoning effort and partial work, and stop SHALL preserve the run's files and slot for explicit continuation. The commands SHALL use the existing executor receipt, task-state, terminal-surface and process-tree owners rather than a parallel executor-management or reporting system, and their results SHALL be available to the calling lead after the run's terminal tab closes.

#### Scenario: Message uses the existing owners
- **WHEN** the lead messages a running pooled executor
- **THEN** addressing, delivery recording and visibility reuse the dispatch receipt, kit-local task state and the run's existing terminal surface rather than a second tracking system

#### Scenario: Stop keeps the slot occupied
- **WHEN** an executor is stopped mid-task with uncommitted changes in its slot
- **THEN** the slot remains bound for the lead's explicit continuation or release decision and the changes are still present

#### Scenario: Instructions name the commands and selection rules
- **WHEN** a new lead session reads the installed kit instructions
- **THEN** executor usage, the delegation guide and the team-lead workflow name executor message, its direct reply form, lead message and the message-versus-stop selection rules, including using the kit's standard commands before manual process killing and preserving partial work after a stop

#### Scenario: Reply does not require manual resume
- **WHEN** a lead answers its executor's unresolved request through the supplied reply reference
- **THEN** the existing live waiting conversation receives the answer and continues in place without requiring the lead to reconstruct an address or run executor resume
