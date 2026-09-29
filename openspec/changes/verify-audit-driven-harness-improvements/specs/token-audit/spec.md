## ADDED Requirements

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
