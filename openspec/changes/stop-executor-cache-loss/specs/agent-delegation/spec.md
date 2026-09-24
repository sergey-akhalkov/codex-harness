## ADDED Requirements

### Requirement: Stop observed DeepSeek executors on sustained cache loss

Observed DeepSeek executor runs SHALL stop automatically when a previously warmed cache suffers three consecutive large misses. Warmup SHALL require a recorded response with at least 100,000 input tokens and at least 90 percent cached input. Each qualifying miss SHALL have at least 100,000 uncached input tokens and at least 50 percent uncached input. A valid intervening response below either miss threshold SHALL reset the consecutive count. Cold starts and duplicate usage records SHALL NOT trigger a stop.

The policy SHALL cover the current control-backed spawn and observed exact-session resume routes. It SHALL use recorded per-response input and cached-input counters for the exact native session, without decoding reasoning or sending model requests for monitoring. Historical usage on explicit resume MAY establish warmup but SHALL NOT count as new misses. Missing or invalid counters SHALL NOT be invented, and unavailable monitoring SHALL be reported.

Monitoring SHALL consume appended data in bounded reads triggered by filesystem/native events and SHALL cover delayed filesystem notifications with a one-second open-file size reconciliation, without periodically rereading unchanged rollout contents. At detection the host SHALL terminate its owned process tree before potentially blocking control acknowledgments or receipt writes, without waiting for model work, tests or builds to finish. Usage only becomes available after a response; this policy SHALL NOT claim a hard spending cap or zero further in-flight requests.

#### Scenario: Filesystem notifications are delayed
- **WHEN** the native writer has appended readable usage but Windows has not delivered a file-change notification
- **THEN** the open-file size reconciliation detects the growth and consumes the appended records while the writer remains open
- **AND** unchanged content is not reread by the reconciliation

#### Scenario: Warmed cache repeatedly fails
- **WHEN** three consecutive distinct responses after warmup cross both miss thresholds
- **THEN** the executor stops its owned child tree, reports the input/cache-miss evidence and retains its session, checkout, slot binding and partial work
- **AND** the receipt and watch result report a non-success outcome and an explicit continuation remedy, without a fallback model or automatic retry

#### Scenario: Cold start or temporary miss
- **WHEN** the cache has not warmed, or a below-threshold response interrupts a sequence of fewer than three qualifying misses
- **THEN** execution continues and the policy does not treat cold input or an isolated loss as a sustained regression

#### Scenario: Resume and identity isolation
- **WHEN** an exact session is resumed and its earlier history includes a cache-loss sequence, or another session's usage is present
- **THEN** historical losses and foreign-session counters do not cause a stop, while new qualifying losses after historical warmup do

#### Scenario: Cleanup is incomplete
- **WHEN** the policy triggers but termination cannot verify that the owned tree is empty
- **THEN** the run reports a partial stop with its cleanup cause instead of claiming that spending has stopped

#### Scenario: No usable usage evidence
- **WHEN** the exact session's per-response usage is unavailable or invalid
- **THEN** the surface and receipt disclose missing coverage, without fabricating cache performance or claiming an enforced spending cap

### Requirement: Recover cache-stopped work in a fresh conversation without losing progress

The cache-stop receipt and watcher SHALL give the lead an actionable fresh-session recovery command for the same verified slot, owner and predecessor session. `executor restart` SHALL preserve the original worktree, commits, uncommitted and untracked files and saved verification evidence without resetting, cleaning or releasing the slot. It SHALL refuse a stale predecessor identity or a live owner. It SHALL start a new conversation with the configured model binding, original assignment and bounded visible progress, retain the predecessor rollout for targeted lookup, and require verification of interrupted commands before reusing results. Missing original assignment SHALL require explicit assignment input rather than inventing a new task. Recovery SHALL NOT resume the old full context or automatically loop after repeated cache failures.

#### Scenario: Stop during a build and continue the original task
- **WHEN** a cache-stopped executor leaves completed edits and an interrupted build, and the lead requests restart for the recorded predecessor
- **THEN** the fresh conversation uses the same worktree and original assignment, receives the bounded visible checkpoint, and checks the saved work and build outcome before continuing
- **AND** the old session remains available and the interrupted build is not reported as successful

#### Scenario: Stale recovery command
- **WHEN** a restart names a predecessor that no longer occupies the slot or its owner is still live
- **THEN** it refuses before model dispatch or worktree mutation instead of replacing the current occupant
