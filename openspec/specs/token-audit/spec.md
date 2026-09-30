# token-audit Specification

## Purpose
Measured local token-usage analysis and an optimization loop for Codex rollout
sessions: reports with context economics, evidence-ranked findings, baseline
diffing and a diagnostic skill consumer.

## Requirements

### Requirement: Measured session reports
The `token-audit report` command SHALL scan local Codex rollout session files
and produce aggregate token reports per session, project, model, reasoning
effort and day from recorded usage events. It SHALL accept both the
`event_msg` token-count format and the `token_usage_record` format, count each
model response once by stable response identity, and report coverage warnings
for malformed, unknown or missing records instead of failing the whole scan.
Reports MUST NOT include conversation transcript content, and project or
workspace paths SHALL be hashed unless the user explicitly supplies private
sources.

#### Scenario: Mixed event formats
- **WHEN** the scanned sessions contain both older token-count events and
  newer token usage records
- **THEN** both contribute once each to the same aggregates and the report
  states per-format coverage

#### Scenario: Malformed line tolerance
- **WHEN** a rollout file contains malformed or unknown event lines
- **THEN** the report completes, counts the affected records under explicit
  coverage warnings, and preserves the rest of the scan's results

#### Scenario: Private paths stay hashed
- **WHEN** a report is produced without explicit private-source input
- **THEN** project and workspace identities appear as stable hashes rather
  than raw local paths

### Requirement: Context economics metrics
Reports SHALL include cache efficiency (cached versus recorded input tokens),
per-session context repayment (the relationship between summed per-turn input
and the final turn's context size), and instruction-floor size (recorded base
and per-turn developer instruction bytes). Turn-level metrics SHALL be omitted
with an explicit unavailable marker when session records lack turn-identified
usage, rather than being synthesized.

#### Scenario: Marathon session repayment
- **WHEN** one session replays a long history across many turns
- **THEN** its report shows the summed per-turn input, the final turn's input
  and the resulting repayment multiplier for that session

#### Scenario: Missing turn identity
- **WHEN** usage records cannot be attributed to individual turns
- **THEN** the report marks turn-level metrics unavailable instead of
  estimating them

### Requirement: Evidence-ranked findings
The `token-audit findings` command SHALL rank optimization findings by
measured token mass and SHALL suppress any finding without measured mass.
Every returned finding SHALL carry a basis label of `measured`, `inferred` or
`estimated`, an evidence locator with session and turn identifiers, an owning
record or skill for remediation, and a validation plan referencing report or
baseline commands. By default only `measured` findings SHALL be returned;
other bases SHALL require an explicit request. Tool-level token attribution
MUST be labeled inferred, because rollout records do not attribute tokens to
individual tools.

#### Scenario: Default basis filter
- **WHEN** findings are requested without extra options
- **THEN** only findings whose basis is measured are returned, and hidden
  inferred or estimated findings appear as counts only

#### Scenario: Owner and validation present
- **WHEN** a finding is returned
- **THEN** it names an existing owning record or skill and a repeatable
  validation command, instead of embedding new instruction text as the fix

### Requirement: Baseline and diff loop
The `token-audit baseline save` command SHALL store report aggregates in
local state under the Codex home harness directory, outside tracked sources,
without transcript content or unhashed private paths. The
`token-audit baseline diff` command SHALL compare a fresh report against a
stored baseline and report metric movement per session, project, model and
day. Stored baselines SHALL either remain readable across analyzer upgrades
or the diff SHALL state the incompatibility explicitly.

#### Scenario: Adopted idea re-measured
- **WHEN** a baseline exists and a new report is diffed after an adopted
  change
- **THEN** the diff shows movement of the chosen validation metrics and names
  the compared baseline

#### Scenario: Baseline stays local
- **WHEN** a baseline is saved
- **THEN** it resides under `CODEX_HOME/harness/token-audit` and contains no
  transcript content

### Requirement: Preserved delegation accounting
Moving rollout event reading into a shared owner SHALL preserve the existing
`codex-harness delegation-usage` command contract: its inputs, outputs,
privacy handling and existing acceptance tests remain valid, and both
commands SHALL use one rollout reader implementation.

#### Scenario: Delegation usage unchanged
- **WHEN** the shared reader replaces the inline parser
- **THEN** the existing delegation-usage tests pass without modification of
  their expectations

### Requirement: Tokenomics skill consumer
The kit SHALL provide a `tokenomics` skill that runs the analyzer, interprets
findings against existing owners, records adoption or rejection in existing
project records, and does not create parallel efficiency instructions. The
skill SHALL be delivered through the kit's normal installation lifecycle and
verified against an installed consumer outside the source checkout.

#### Scenario: Diagnostic run
- **WHEN** the skill is invoked where recorded sessions exist
- **THEN** it produces the analyzer report, ranked findings mapped to owning
  records, and a validation step for each proposed idea

#### Scenario: Installed consumer
- **WHEN** the delivered kit is installed outside the checkout
- **THEN** the skill resolves and runs the analyzer from its installed
  location

### Requirement: No proxy pricing or quota claims
Analyzer outputs MUST NOT present currency costs or quota percentages. Token
counts, byte counts and explicit coverage SHALL be the only quantitative
currencies, and any estimate SHALL remain labeled as an estimate.

#### Scenario: Subscription session
- **WHEN** sessions were produced through subscription routing without usable
  rate-limit records
- **THEN** the report shows recorded tokens and coverage only, without
  invented cost or allowance fields

### Requirement: Bounded report presentation with complete detail
Interactive report and findings output SHALL present bounded ranked records, total and omitted counts, coverage warnings and the measurement limitations. Full existing JSON and baseline contracts MUST remain compatible. A compact observation SHALL retain its complete machine report in local evidence and provide detail access without scanning sessions again. Retention SHALL be bounded and expired detail SHALL produce an explicit error. Presentation MUST NOT alter aggregate counters or convert bytes into claimed subscription savings.

#### Scenario: Many recorded sessions
- **WHEN** the report contains more rows than the presentation limit
- **THEN** the summary reports omitted counts and a locator for the complete same-scan report
- **AND** retrieving details does not run a model or rescan sessions

#### Scenario: Incomplete or erroneous usage
- **WHEN** coverage is incomplete or some usage is unavailable
- **THEN** the summary retains that warning and does not present missing values as zero

#### Scenario: Existing machine consumer
- **WHEN** a caller explicitly requests full JSON or a baseline operation
- **THEN** its complete existing data contract is preserved

#### Scenario: Two scans finish with the same timestamp
- **WHEN** two reports share a generated timestamp but contain different observations
- **THEN** each returned locator retains its own complete scan without overwriting the other

### Requirement: Immutable uniquely named baseline publication

Baseline save SHALL publish a uniquely named complete snapshot and return an identifier accepted unchanged by baseline diff. Resolution SHALL accept the documented basename and filename forms and keep relative names and latest pointers inside the owned baseline directory. The default directory SHALL resolve the same Codex home as other harness state, including the user-profile fallback. Concurrent and interrupted saves SHALL preserve every successfully returned snapshot and a latest pointer naming a complete committed snapshot; latest ordering SHALL be defined by publication rather than an assumed timestamp ordering. Baseline files SHALL remain local and free of transcript content and unhashed private paths.

#### Scenario: Returned name is used immediately
- **WHEN** a caller passes the name returned by save to diff, with or without the documented JSON suffix
- **THEN** it resolves that snapshot without adding a duplicate suffix

#### Scenario: Two saves occur in one clock second
- **WHEN** separate processes save different snapshots with identical report timestamps
- **THEN** both returned identities remain distinct and readable and latest points to one complete published snapshot

#### Scenario: Writer exits during publication
- **WHEN** a save is interrupted before snapshot or latest publication completes
- **THEN** existing snapshots remain valid, latest never authorizes a partial snapshot, and recovery does not overwrite a different writer's evidence

#### Scenario: Codex home is not explicit
- **WHEN** CODEX_HOME is unset and a user profile is available
- **THEN** default baseline state resolves beneath that profile's native Codex home rather than a sibling harness directory

#### Scenario: A pointer escapes its directory
- **WHEN** a requested relative name or latest pointer attempts to address an absolute or parent-relative path
- **THEN** resolution rejects it and does not interpret unrelated local files as owned baselines

### Requirement: Baseline validity comparability and coverage remain distinct

A baseline SHALL be structurally validated before it can be marked format-compatible. Comparison SHALL separately report snapshot validity, format compatibility and scope comparability, including source identity, window definition, accounting mode, analyzer semantics and material coverage differences. Unsupported or malformed snapshots SHALL provide an explicit reason without fabricated zero baselines or improvement deltas. Partial usage, recorded subtotals, missing data, warning counts and usage bases SHALL survive snapshotting and comparison. A coverage change SHALL NOT be interpreted as a resource saving. Legacy snapshots lacking required comparison metadata SHALL remain inspectable but SHALL NOT silently qualify for stronger claims.

#### Scenario: Only a valid schema number is present
- **WHEN** JSON has the expected schema number but lacks required snapshot fields
- **THEN** it is invalid rather than compatible and no valid-baseline comparison is claimed

#### Scenario: Two valid snapshots describe different populations
- **WHEN** roots, time semantics, windows or other material scope properties differ
- **THEN** format validity remains distinguishable from comparability and an ordinary movement report cannot claim a controlled improvement

#### Scenario: Usage disappears from part of the population
- **WHEN** a lower current subtotal accompanies missing or corrupt usage that was present in the baseline
- **THEN** the report exposes the changed coverage and unknown remainder and does not label the lower subtotal a saving

### Requirement: Bounded baseline presentation retains complete detail

Default text diff SHALL use documented deterministic row and byte bounds, display complete aggregate totals with their coverage, summarize significant changes and identify the number of omitted rows. Stable detail access SHALL expose any omitted session or group without requiring an unbounded default response or changing the compared inputs. Explicit full machine output SHALL remain available. Pagination and sorting SHALL neither lose nor duplicate rows.

#### Scenario: Thousands of sessions change
- **WHEN** a diff contains thousands of session and project/model/day movements
- **THEN** default text stays inside its declared bounds, omitted counts are correct and totals agree with full machine output

#### Scenario: An omitted row is needed
- **WHEN** a caller follows a detail locator for an omitted movement
- **THEN** that movement comes from the same comparison, or expiration is explicit rather than replaced with a silently different scan

### Requirement: Explicit session activity and interval usage accounting

The analyzer SHALL distinguish the lifetime totals of sessions active in a window from usage attributable to a declared interval. Existing activity-window behavior SHALL remain explicitly labeled for compatibility. Interval reports SHALL state UTC half-open boundaries, timestamp provenance, accounting basis and missing coverage. Stable response identities SHALL deduplicate repeated records while distinct retries remain counted. Cumulative counters SHALL contribute only identifiable increments under explicit reset, gap and boundary rules; an unidentifiable interval allocation stays unknown. Model and effort attribution SHALL follow supported event evidence rather than assigning a mixed session to a guessed model. Session-start day grouping SHALL NOT be described as daily spending. Both modes SHALL preserve the shared reader's delegation-accounting contract.

#### Scenario: An old session receives one new response
- **WHEN** a session recorded 100000 tokens before the interval and an attributable 1000-token response inside it
- **THEN** the activity report can show its 101000 lifetime total while the interval report shows only the attributable 1000 and states its accounting basis

#### Scenario: A cumulative delta crosses an unresolved boundary
- **WHEN** cumulative observations do not identify how much usage belongs on either side of the interval boundary, or a counter resets ambiguously
- **THEN** the uncertain amount stays unavailable instead of being charged entirely to the interval or treated as zero

#### Scenario: Duplicate response and genuine retry
- **WHEN** an event repeats a stable response identity and another event represents a distinct retry
- **THEN** the duplicate is counted once, the retry remains counted and conflicting records receive a coverage warning

#### Scenario: Model changes or timing evidence is missing
- **WHEN** a session changes models or lacks usable event timestamps
- **THEN** supported attribution is retained per identifiable usage unit and the remainder stays explicitly unassigned

### Requirement: Incremental analysis preserves full-scan meaning

An enabled incremental analyzer SHALL produce the same accepted report, coverage and findings as the full reader for the same complete input snapshot and options, excluding explicit operational timing/I/O counters. Cached state SHALL be local, versioned, bounded and disposable. Reuse SHALL require evidence that the already parsed prefix and parser semantics remain valid; path, length and modification time alone SHALL NOT establish this property. Uncertain identity, replacement, truncation, unsupported mutation or parser-version change SHALL cause safe full parsing. Active files SHALL be consumed only through complete events and a declared snapshot boundary. Output SHALL report bytes read, reused inputs and invalidation reasons, without inferring an unmeasured latency improvement.

#### Scenario: An active file appends a partial line
- **WHEN** an append ends inside an event and later completes it
- **THEN** the first scan records the partial coverage and the next scan consumes the completed event exactly once, matching full parsing of each snapshot

#### Scenario: A file changes without a useful stat difference
- **WHEN** a file is replaced or an earlier byte range changes while its size and timestamp are preserved
- **THEN** reuse requires an independent valid change/immutability proof; otherwise a full scan prevents stale usage results

#### Scenario: Parser or accounting mode changes
- **WHEN** cached parser semantics or requested accounting options differ materially
- **THEN** incompatible cached state is invalidated and no prior interpretation is silently reused

#### Scenario: Unchanged eligible inputs are reused
- **WHEN** the unchanged-prefix contract is established for previously parsed files
- **THEN** repeated analysis reads fewer source bytes while its semantic result matches the full-scan reference
