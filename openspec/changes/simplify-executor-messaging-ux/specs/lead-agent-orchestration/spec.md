## ADDED Requirements

### Requirement: Size-free message delivery in both directions

The kit's executor message and lead message commands SHALL accept message
content from three interchangeable literal sources: a short `--text TEXT`
argument, a UTF-8 `--file FILE`, and the full UTF-8 content of standard input
when it is piped. Reading standard input SHALL NOT require the caller to
know or name any size limit, and an interactive terminal with no content
argument SHALL be refused harmlessly rather than blocking. A message whose
composed payload exceeds the inline delivery bound SHALL be delivered
automatically: the command persists the complete literal payload, without
truncation or shell evaluation, into a harness-owned message file, and
delivers a compact pointer envelope into the same live conversation stating
the message identity, the exact byte size and the absolute path of that file
so the recipient can read the full message with its ordinary tools. Spilled
delivery SHALL be recorded as its own honest result class with the spill
file's absolute path in the existing message receipts and watch output, SHALL
NOT be presented as inline delivery, and a payload above the fixed spill
ceiling SHALL be refused with the actual ceiling and nothing partially sent.
Spill files SHALL live in the recorded harness message state rather than a
worktree or repository, SHALL be cleaned together with their owning run
records, and their directory SHALL stay bounded. This mechanism SHALL apply
identically to lead-to-executor steering, executor-to-lead messages and
reply-reference replies, and SHALL NOT alter the existing queued, delivered
and error result classes for inline-sized messages.

#### Scenario: A long correction is delivered without sender effort
- **WHEN** the lead pipes a multiline correction larger than the inline bound into the executor message command while its executor runs
- **THEN** the command itself persists the complete payload, delivers a pointer envelope into the executor's live conversation naming the exact size and absolute file path, and reports the spilled delivery with that path instead of refusing or truncating

#### Scenario: Executor question travels by standard input
- **WHEN** an executor pipes its clarification question into the lead message command without any content flag
- **THEN** the piped UTF-8 content is delivered as the literal message with the same metadata, reply reference and waiting behavior as a `--text` message, and no manual file creation is involved

#### Scenario: The recipient reads the full message
- **WHEN** a spilled pointer envelope arrives in a live conversation
- **THEN** the recipient can read the complete original message from the named absolute path with its ordinary file tools, and the watch and receipt surfaces show the spill path and size

#### Scenario: An oversized payload is refused honestly
- **WHEN** a piped payload exceeds the spill ceiling
- **THEN** the command refuses with the actual ceiling, sends nothing partially, and leaves no phantom delivery record or reply hold

#### Scenario: No piped input is not a silent empty message
- **WHEN** either message command runs without a content flag and standard input is an interactive terminal
- **THEN** the command refuses harmlessly with the supported content sources instead of blocking or sending an empty message

### Requirement: Recorded-identity address defaults

The lead-side executor commands SHALL accept the recorded checkout, home,
slot, owner and session address fields as optional. When an address field is
omitted, the command SHALL resolve it only from the harness's own recorded
state - the effective `CODEX_HOME` or its recorded installation default, the
installation record's source checkout, and the recorded live pool lease,
receipt and session identity - and SHALL then apply the same identity
verification it applies to explicitly supplied values. When exactly one live
run matches, that run SHALL be addressed automatically; when several live
runs match, the command SHALL refuse with a bounded listing of the live runs
and the disambiguating slot argument; when none matches, it SHALL report the
same honest state error as today. Resolution SHALL NOT infer an address from
the current working directory, process names, window titles or the most
recent session, and an explicitly supplied value SHALL always take precedence
over a resolved default. The existing explicit address surface SHALL remain
accepted and unchanged in meaning, and installation-lifecycle commands SHALL
keep requiring their explicit paths.

#### Scenario: One live executor needs no address
- **WHEN** the lead messages, watches or stops its only live executor run using no address fields
- **THEN** the command resolves that run from the recorded pool identity, verifies it as if the fields were typed, and delivers or acts on exactly that run

#### Scenario: Several live runs demand a slot
- **WHEN** two or more live runs match the omitted address fields
- **THEN** the command refuses to choose, lists the live runs with their slots, and names the slot argument that disambiguates

#### Scenario: Defaults never weaken verification
- **WHEN** a resolved default points at a slot whose recorded session no longer matches the live run
- **THEN** the command refuses with the identity mismatch exactly as it would for explicit values, and nothing is delivered to the current occupant

#### Scenario: Explicit paths remain authoritative
- **WHEN** the lead supplies `--source`, `--codex-home`, `--slot` or `--owner` explicitly
- **THEN** those values are used and verified unchanged, and no recorded default overrides them

### Requirement: Standard-input assignment text for executor spawn

Executor spawn SHALL accept `--exec -` to read the complete free-text
assignment from standard input as literal UTF-8, so a long assignment does
not depend on process command-line length limits. The streamed assignment
SHALL be rendered and dispatched with the same brief, boundaries, base
revision and pool isolation as an equal `--exec TEXT` value, and spawn SHALL
refuse harmlessly when `--exec -` meets an interactive terminal or an empty
stream. `--exec TEXT` and `--assignment FILE` SHALL keep their current
meaning.

#### Scenario: A long assignment is spawned by pipe
- **WHEN** the lead pipes a long free-text assignment into spawn with `--exec -`
- **THEN** the executor receives the complete literal assignment with the standard rendered brief and isolation, and no command-line length limit is involved

#### Scenario: Empty stdin assignment is refused
- **WHEN** spawn runs with `--exec -` and standard input is an interactive terminal or an empty stream
- **THEN** spawn refuses with the supported assignment sources and allocates no slot

## MODIFIED Requirements

### Requirement: Lead steering and executor escalation without waste

The lead SHALL deliver steering to an executor through the kit's addressed executor message command into that executor's live conversation, and the delivered input SHALL appear in that executor's visible conversation. The message command SHALL address one existing run through the accepted checkout, slot, owner and exact recorded session identity - supplied explicitly or resolved from recorded state when exactly one live run matches - SHALL verify that identity against the actual run before delivery so a message cannot reach a later occupant of a reused slot, and SHALL accept a short literal text, a UTF-8 file containing multiline text, or the full UTF-8 content of standard input, preserving real line breaks and literal content without shell evaluation regardless of size up to the spill ceiling. Delivery SHALL go to the same conversation with its context, model, provider, reasoning effort and completed work preserved, and SHALL NOT be implemented through hidden stop/resume, a new conversation, model replacement or re-sending the whole task. While an executor is working, delivery SHALL use the backend's real capability to accept additional input, including at the nearest supported point during a running tool call, and SHALL NOT interrupt that tool call. The command SHALL distinguish queued input, confirmed delivery, spilled delivery and error, SHALL NOT present a local file write as delivery to the model, and a retry after an indeterminate result SHALL NOT silently deliver the same context twice. For a completed, stopped or unavailable run it SHALL return an explicit result with a supported next action and SHALL NOT start a new conversation. Waiting and routine event handling SHALL require no model calls, and no repeated status requests SHALL be sent to active executors. Steering SHALL add relevant facts, resolve a request, or correct an established mistake; status-only nudges, hurry demands and repeated messages without new facts SHALL NOT be sent. Executor reports and escalations SHALL reach the lead as board feedback tasks with bounded context rather than unbounded transcript copies.

#### Scenario: A mid-assignment correction arrives
- **WHEN** the lead sends a concrete correction while an executor is working
- **THEN** the input appears in that executor's conversation, the executor continues in the same session and worktree, and no duplicate session is started

#### Scenario: A path correction is delivered verbatim
- **WHEN** the lead sends a multiline UTF-8 correction from a file while the executor is inside a long tool call
- **THEN** the message is delivered at the nearest supported point without interrupting that tool call, its literal text appears in the executor's conversation, and the reported result distinguishes delivery from later application by the executor

#### Scenario: A correction is typed without any address
- **WHEN** the lead pipes a correction into the executor message command with no address fields while exactly one executor run is live
- **THEN** the command resolves and verifies that run from recorded state and delivers the correction to its conversation without the lead copying any checkout, home, slot, owner or session value

#### Scenario: A stale identity is refused
- **WHEN** the addressed slot no longer runs the recorded session, or the slot was reused by another run
- **THEN** message refuses with the observed identity mismatch and names a supported next action instead of delivering to the current occupant

#### Scenario: A finished run cannot be messaged
- **WHEN** message addresses a run whose recorded lifecycle is completed, stopped or unavailable
- **THEN** the command returns that actual state with the continuation remedy and starts no new conversation

#### Scenario: A healthy executor is waiting
- **WHEN** an executor remains active beyond several observation waits without a demonstrated failure
- **THEN** waiting generates no model status requests and the executor's ownership is preserved

#### Scenario: A nudge without new facts is not sent
- **WHEN** the lead is tempted to ask for status or hurry an executor that has no new decision-relevant fact
- **THEN** no message is sent, and steering is reserved for concrete corrections, facts or requirement changes
