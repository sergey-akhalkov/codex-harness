## ADDED Requirements

### Requirement: Addressed executor message and stop commands

The delegation kit SHALL expose an executor message command and an executor stop command that address one pooled executor run through the accepted checkout, slot, owner and exact recorded session identity. Both commands SHALL verify the addressed run's actual identity and lifecycle before acting, SHALL behave deterministically for completed, stopped, interrupted or unavailable runs by reporting the observed state with a supported next action, and SHALL NOT start a new conversation, reset or clean the slot, or release it automatically. Message delivery SHALL preserve the run's conversation, model, provider, reasoning effort and partial work, and stop SHALL preserve the run's files and slot for explicit continuation. The commands SHALL use the existing executor receipt, task-state, terminal-surface and process-tree owners rather than a parallel executor-management or reporting system, and their results SHALL be available to the calling lead after the run's terminal tab closes.

#### Scenario: Message uses the existing owners
- **WHEN** the lead messages a running pooled executor
- **THEN** addressing, delivery recording and visibility reuse the dispatch receipt, kit-local task state and the run's existing terminal surface rather than a second tracking system

#### Scenario: Stop keeps the slot occupied
- **WHEN** an executor is stopped mid-task with uncommitted changes in its slot
- **THEN** the slot remains bound for the lead's explicit continuation or release decision and the changes are still present

#### Scenario: Instructions name the commands and selection rules
- **WHEN** a new lead session reads the installed kit instructions
- **THEN** the executor usage, delegation guide and team-lead workflow name both commands and the message-versus-stop selection rules, including using the kit's standard commands before manual process killing and preserving partial work after a stop

