## ADDED Requirements

### Requirement: Minimal delegation command forms in guidance

The kit's generated executor guidance, team-lead workflow, CLI help and
delegation reference SHALL teach the minimal command forms as the first
choice: message content piped on standard input, automatic delivery of
oversized messages through the harness-owned spill, and lead-side addressing
with no copied checkout, home, slot, owner or session values when recorded
state identifies the run. Guidance SHALL state that no message size or
transport limit needs to be known before sending, SHALL name the explicit
forms as fallbacks for disambiguation and scripting, and SHALL keep the
existing message-versus-stop selection rules unchanged. Replacing path-heavy
instructions with the minimal forms SHALL NOT grow the combined required
guidance load across generated briefs, the team-lead skill and required
references, and no instruction surface SHALL require a caller to construct a
temporary file to send a long message.

#### Scenario: A lead steers without copying addresses
- **WHEN** a lead session reads the installed workflow guidance and must correct its single live executor
- **THEN** the documented first-choice command pipes the correction with no address fields and no content file, and the guidance identifies the explicit address flags only as the disambiguation and scripting form

#### Scenario: An executor asks without size knowledge
- **WHEN** an executor consults its injected brief before escalating a material ambiguity to the lead
- **THEN** the brief shows the piped or short-text message form without any file-creation step, size rule or transport detail, and preserves the existing exceptional-use boundaries

#### Scenario: Guidance does not grow
- **WHEN** the minimal forms replace the prior path-heavy command examples across the generated brief, skill and references
- **THEN** the combined required guidance load for the same assignment inputs is not larger than before the change

## MODIFIED Requirements

### Requirement: Addressed executor message and stop commands

The delegation kit SHALL expose an executor message command and an executor stop command that address one pooled executor run through the accepted checkout, slot, owner and exact recorded session identity, each of which MAY be supplied explicitly or resolved from recorded state when omission leaves exactly one live run. Executor message SHALL additionally accept a verified `--reply-to MESSAGE_ID` that resolves the original sender without those manual address fields. Both commands SHALL verify the addressed run's actual identity and lifecycle before acting, SHALL behave deterministically for completed, stopped, interrupted or unavailable runs by reporting the observed state with a supported next action, and SHALL NOT start a new conversation, reset or clean the slot, or release it automatically. A waiting-for-reply run SHALL remain live and directly addressable, and a correlated reply SHALL continue that same conversation without a separate resume command. Message content SHALL be accepted as a short literal text, a UTF-8 file containing multiline text, or the full UTF-8 content of piped standard input, and a payload above the inline delivery bound SHALL be delivered automatically through the harness-owned spill pointer rather than refused. Message delivery SHALL preserve the run's conversation, model, provider, reasoning effort and partial work, and stop SHALL preserve the run's files and slot for explicit continuation. The commands SHALL use the existing executor receipt, task-state, terminal-surface and process-tree owners rather than a parallel executor-management or reporting system, and their results SHALL be available to the calling lead after the run's terminal tab closes.

#### Scenario: Message uses the existing owners
- **WHEN** the lead messages a running pooled executor
- **THEN** addressing, delivery recording and visibility reuse the dispatch receipt, kit-local task state and the run's existing terminal surface rather than a second tracking system

#### Scenario: Message addresses a unique live run with no flags
- **WHEN** the lead runs the executor message command with only piped content while exactly one pooled run is live
- **THEN** the command resolves that run from the recorded pool identity and delivers into its exact live conversation

#### Scenario: Reply does not require manual resume
- **WHEN** a lead answers its executor's unresolved request through the supplied reply reference
- **THEN** the existing live waiting conversation receives the answer and continues in place without requiring the lead to reconstruct an address or run executor resume

#### Scenario: Stop keeps the slot occupied
- **WHEN** an executor is stopped mid-task with uncommitted changes in its slot
- **THEN** the slot remains bound for the lead's explicit continuation or release decision and the changes are still present

#### Scenario: Instructions name the commands and selection rules
- **WHEN** a new lead session reads the installed kit instructions
- **THEN** the executor usage, delegation guide and team-lead workflow name both commands and the message-versus-stop selection rules, including using the kit's standard commands before manual process killing and preserving partial work after a stop
