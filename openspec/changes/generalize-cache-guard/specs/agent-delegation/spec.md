## RENAMED Requirements

- FROM: `### Requirement: Stop observed DeepSeek executors on sustained cache loss`
- TO: `### Requirement: Stop observed executors on runtime-proven sustained cache loss`

## MODIFIED Requirements

### Requirement: Stop observed executors on runtime-proven sustained cache loss

Observed managed executor runs SHALL stop automatically when a previously warmed cache suffers three consecutive large misses, regardless of the configured provider or model. Warmup SHALL require a recorded response with at least 100,000 input tokens and at least 90 percent cached input, and SHALL be the runtime proof that this policy applies to that run. The mere presence of a cached-input field, provider branding, or a static model allowlist SHALL NOT establish support. Each qualifying miss SHALL have at least 100,000 uncached input tokens and at least 50 percent uncached input. A valid intervening response below either miss threshold SHALL reset the consecutive count. Cold starts, never-warmed runs, and duplicate usage records SHALL NOT trigger a stop.

The policy SHALL cover the current control-backed spawn and observed exact-session resume routes for every configured executor profile. It SHALL use recorded per-response input and cached-input counters for the exact native session, without decoding reasoning or sending model requests for monitoring. Historical usage on explicit resume MAY establish warmup but SHALL NOT count as new misses. Missing or invalid counters SHALL NOT be invented; unavailable, invalid, or never-proven coverage SHALL be reported with the run's actual resolved provider and model. Monitoring SHALL consume appended data in bounded reads triggered by filesystem/native events and SHALL cover delayed filesystem notifications with a one-second open-file size reconciliation, without periodically rereading unchanged rollout contents. At detection the host SHALL terminate its owned process tree before potentially blocking control acknowledgments or receipt writes, without waiting for model work, tests or builds to finish. Usage only becomes available after a response; this policy SHALL NOT claim a hard spending cap or zero further in-flight requests.

#### Scenario: Filesystem notifications are delayed
- **WHEN** the native writer has appended readable usage but Windows has not delivered a file-change notification
- **THEN** the open-file size reconciliation detects the growth and consumes the appended records while the writer remains open
- **AND** unchanged content is not reread by the reconciliation

#### Scenario: A non-DeepSeek provider proves cache warmup
- **WHEN** an executor on any configured provider records a response meeting the warmup thresholds and then three consecutive distinct responses cross both miss thresholds
- **THEN** the executor stops its owned child tree and reports the provider/model, input, cached-input, and miss evidence with the same partial-work guarantees as any other supported run

#### Scenario: Warmed cache repeatedly fails
- **WHEN** three consecutive distinct responses after warmup cross both miss thresholds
- **THEN** the executor stops its owned child tree, reports the input/cache-miss evidence and retains its session, checkout, slot binding and partial work
- **AND** the receipt and watch result report a non-success outcome and an explicit continuation remedy, without a fallback model or automatic retry

#### Scenario: Cold start or temporary miss
- **WHEN** the cache has not warmed, a below-threshold response interrupts a sequence of fewer than three qualifying misses, or numeric counters never establish a warmed response
- **THEN** execution continues and the policy does not treat cold input, an isolated loss, or unproven support as a sustained regression

#### Scenario: Resume and identity isolation
- **WHEN** an exact session is resumed and its earlier history includes a cache-loss sequence, or another session's usage is present
- **THEN** historical losses and foreign-session counters do not cause a stop, while new qualifying losses after historical warmup do

#### Scenario: Cleanup is incomplete
- **WHEN** the policy triggers but termination cannot verify that the owned tree is empty
- **THEN** the run reports a partial stop with its cleanup cause instead of claiming that spending has stopped

#### Scenario: No usable usage evidence
- **WHEN** the exact session's per-response usage is unavailable or invalid
- **THEN** the surface and receipt disclose missing coverage, without fabricating cache performance or claiming an enforced spending cap

## ADDED Requirements

### Requirement: Cache-guard diagnostics use the native warning surface

The managed executor host SHALL distinguish routine cache-guard status, degraded cache-guard coverage, and a terminal cache-loss stop. Routine status transitions SHALL be recorded in the run receipt and host log without becoming user-facing warnings. Degraded coverage diagnostics SHALL be delivered through Codex's native warning presentation when a native frontend is attached, so they participate in the frontend's warning count and viewer rather than being written directly into the terminal input area. The host SHALL record whether each degraded diagnostic was delivered natively, could not be delivered, or had no native warning consumer. A delivery failure SHALL NOT be reported as a shown warning, and a compatibility surface without a native warning consumer SHALL use only its existing receipt/log surface.

A terminal cache-loss stop SHALL retain its non-success outcome, stop cause, bounded evidence, and explicit recovery command; it SHALL NOT be represented as a dismissible warning that replaces the existing result.

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
- **THEN** the native surface, receipt, and watch result report the stop cause and recovery path as a non-success terminal outcome
- **AND** that outcome is not reduced to a warning-list entry without result state
