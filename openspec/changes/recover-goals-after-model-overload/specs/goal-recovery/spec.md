# Goal Recovery Spec Delta

## Purpose

Keep an unfinished native Codex goal progressing through transient model overload while preserving the user's control, existing work and resource limits.

## ADDED Requirements

### Requirement: Recovery eligibility

The harness SHALL consider automatic recovery only when an owned, previously active goal's latest completed turn failed with native `serverOverloaded` and the same goal is now blocked. It MUST establish the failure and current state from native structured events and reads. Unknown state or an unrelated error MUST NOT authorize recovery.

#### Scenario: Temporary overload ends useful work
- **WHEN** a supported session's active goal becomes blocked after its latest turn fails with `serverOverloaded`
- **THEN** the harness schedules bounded recovery for that goal without requiring `/goal resume`

#### Scenario: An unrelated stopping condition
- **WHEN** a goal is absent, complete, manually paused, usage limited, budget limited, blocked by the agent, or stopped by authentication, permission, policy, invalid-input or another error
- **THEN** overload recovery does not activate it or create another goal

#### Scenario: Stale evidence on attachment
- **WHEN** an observer attaches to a previously blocked session without an owned pending recovery episode, or receives an overload event for an older turn
- **THEN** the event does not authorize a new automatic recovery episode

### Requirement: Bounded recovery episodes

Recovery SHALL use exponential backoff with bounded jitter and at most five automatic resume attempts per consecutive overload episode. It MUST honor an available native retry deadline. Duplicate events, reconnection and successful intermediate requests MUST NOT replenish attempts. A successfully completed goal turn or explicit user resume starts a new episode.

#### Scenario: Service recovers during the episode
- **WHEN** an automatic resume produces a successfully completed turn
- **THEN** that episode ends, native goal pursuit continues as applicable, and a later independent overload can receive its own bounded episode

#### Scenario: Overload persists
- **WHEN** all five automatic resumes fail with overload
- **THEN** the goal remains blocked, no sixth automatic resume is sent, and the terminal identifies exhaustion and the manual continuation command

#### Scenario: Repeated delivery and partial progress
- **WHEN** a failed turn is reported more than once, the observer reconnects, or a retry performs tools before failing again
- **THEN** the same failed turn authorizes at most one resume and the existing episode's remaining attempts are preserved

### Requirement: Goal and accounting preservation

Recovery SHALL update the existing goal's status without replacing its objective, resetting accounting, increasing its budget or changing its thread. It MUST retain model, provider, reasoning configuration, credentials and billing route. Recovery MUST NOT bypass native usage or budget limits.

#### Scenario: Resume a partly completed goal
- **WHEN** the blocked goal has an existing budget and accumulated usage
- **THEN** recovery preserves its identity, objective, budget and prior accounting, and new work contributes to the same accounting

#### Scenario: A limit changes during the wait
- **WHEN** the goal reaches a usage or budget limit before a pending resume is admitted
- **THEN** the attempt is cancelled without changing the limit or switching model or billing route

### Requirement: Work continuity

Recovery SHALL retain the existing native conversation and running tool/process state. It MUST NOT replay the original user prompt, recreate the thread, restart the CLI, repeat completed tool effects or discard pending results as its recovery mechanism.

#### Scenario: Completed effect before overload
- **WHEN** a tool changes an owned file once and the next model response fails with overload
- **THEN** recovery continues with that result available and does not repeat the file change

#### Scenario: Tool work remains in flight
- **WHEN** work started before the failure remains in flight or a background terminal remains active
- **THEN** recovery preserves that work and the same conversation can consume its native result without restarting the process or treating every long-lived terminal as a blocker

### Requirement: Authoritative user control

User pause, interruption, new input, goal replacement or clearing, and session closure SHALL invalidate pending recovery. Control actions and recovery dispatch MUST have an observable ordering: a user action admitted before a resume is forwarded wins. A later action retains native semantics and cancels further automatic attempts. A separate status read followed by an unchecked activation MUST NOT satisfy this requirement.

#### Scenario: Pause while recovery is waiting
- **WHEN** `/goal pause` is admitted before the scheduled resume is forwarded
- **THEN** the goal remains paused and no automatic activation or model request follows from the cancelled attempt

#### Scenario: Pause races with the final status read
- **WHEN** the observer read a blocked goal but a user pause is admitted before its activation command is forwarded
- **THEN** the stale activation is discarded despite the earlier eligible snapshot

#### Scenario: New work supersedes the failed turn
- **WHEN** a user submits input, replaces or clears the goal, or starts a newer turn before the scheduled activation
- **THEN** the earlier failure cannot activate, overwrite or inject work into the new state

#### Scenario: Pause after recovery was dispatched
- **WHEN** the native runtime accepted the recovery before the user pauses or interrupts
- **THEN** the user's command is delivered with its native meaning, pending further retries are cancelled, and the outcome is not falsely reported as cancellation before dispatch

### Requirement: Owned recovery lifetime

Each live session SHALL have at most one recovery owner. Losing authoritative control, closing the frontend, or ending the owner MUST cancel pending dispatch. Reconnection MUST reconcile native state before any further action and MUST NOT blindly replay an unacknowledged resume. Recovery MUST NOT use direct database writes, screen scraping or simulated desktop keystrokes.

#### Scenario: Connection drops during activation
- **WHEN** the connection fails after a resume might have reached Codex but before its acknowledgement arrives
- **THEN** the owner reports an uncertain result and reconciles the goal and latest turn before proceeding, without issuing a duplicate resume

#### Scenario: The user leaves the session
- **WHEN** the owned terminal or session closes while recovery is waiting
- **THEN** no delayed command reopens the session or starts hidden model work, and owned processes are reclaimed without affecting other sessions

### Requirement: Ordinary launch compatibility

Enabled recovery SHALL be available through the installed ordinary interactive `codex` and `codex resume` routes without a special launch command. It MUST preserve the caller's terminal, arguments, workspace, configuration precedence and native exit behavior. It MUST NOT arrange desktop windows, activate delegation, change providers or require a machine-wide watcher service.

#### Scenario: First use and repeat use outside the kit
- **WHEN** a user starts and later reopens a session through the global launcher in another project
- **THEN** new eligible overloads receive recovery in that same terminal with the selected configuration, and prior manual stopping conditions are not automatically revived

#### Scenario: Recovery cannot be prepared
- **WHEN** required native control capabilities are unavailable before a session starts
- **THEN** the ordinary CLI still starts once with a clear recovery-unavailable notice and unchanged user settings, and the launcher does not build, install or silently retarget the session

#### Scenario: An existing managed session owner
- **WHEN** the harness already owns a supported hosted session
- **THEN** recovery uses that owner instead of creating a second backend, observer authority or frontend

### Requirement: Visible recovery state

The existing terminal SHALL identify the overload, scheduled attempt and delay, recovery, cancellation, exhaustion or observation failure. Messages MUST distinguish waiting from active work and expose a safe next action when automatic recovery ends. Diagnostics MUST remain bounded and keep raw session data and credentials in private local storage.

#### Scenario: Waiting for capacity
- **WHEN** recovery waits before an attempt
- **THEN** the user can see which attempt is pending, when it is due and how to stop it without opening logs or another window

#### Scenario: Native status remains blocked during the wait
- **WHEN** the native UI reports a stalled goal while the owner is waiting
- **THEN** the recovery message explains that an automatic retry is pending, without falsely reporting the goal as active or completed

### Requirement: Reversible global delivery

Recovery SHALL participate in existing install, update, check, disable, disconnect and rollback lifecycles. Activation MUST preserve unrelated configuration, hooks and live sessions. Check MUST distinguish configured support from exercised runtime behavior. Sessions launched with recovery disabled MUST perform no automatic attempts; live user cancellation MUST remain immediate.

#### Scenario: Install and rollback
- **WHEN** recovery is installed or updated and subsequently disabled or rolled back
- **THEN** new sessions receive the selected behavior, running sessions remain owned and intact, and goals, credentials, unrelated hooks and configuration survive

### Requirement: Native acceptance evidence

Acceptance SHALL exercise overload and recovery through installed native components and the ordinary global entry point in isolated owned state outside this checkout. Synthetic Responses MUST establish deterministic failure and success without paid model calls. Discovery, passing component checks and delivered integrated behavior MUST be reported separately.

#### Scenario: Required native recovery matrix
- **WHEN** the capability is declared delivered
- **THEN** retained checks establish same-thread recovery, accounting and tool-effect preservation, bounded exhaustion, user-control races, ambiguous acknowledgements, closure, repeat use and lifecycle rollback, with current runtime identities and complete owned cleanup

#### Scenario: Discovery has not implemented the owner
- **WHEN** a disposable probe demonstrates native API recovery but the ordinary launcher and cancellation owner are not implemented
- **THEN** that evidence supports the design but does not close implementation or global acceptance tasks
