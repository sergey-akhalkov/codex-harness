## MODIFIED Requirements

### Requirement: Enforced bounded indexing

Every CodeGraph full-index or incremental-refresh operation launched through the kit, including watcher and connect-time work, SHALL have a finite execution deadline, one account-wide indexing admission slot and enforced Windows process-tree limits. The initial policy SHALL use one parse worker and one resolution worker, at most 2 GiB aggregate committed memory, 25 percent of host CPU and a 600-second execution deadline. Auxiliary workers SHALL remain within the same process-tree budget. These limits SHALL NOT multiply with the number of CLI clients or Codex homes. Internal advisory budgets MUST NOT be represented as OS enforcement. Concurrent requests SHALL wait within their deadline or receive an explicit busy result with the affected project and pending state. Admission SHALL schedule pending projects fairly; holding a session open or continually editing one project MUST NOT reserve the indexing slot indefinitely or prevent other active projects from refreshing. Resource denial, timeout, cancellation and incomplete indexing SHALL preserve the previously committed index and SHALL NOT trigger unbounded retries, a higher memory limit or an unsupervised fallback. Read operations and their owned workers SHALL also have finite memory, execution and idle limits.

#### Scenario: The indexer ignores its internal budget
- **WHEN** a supervised worker attempts allocations beyond the configured hard limit
- **THEN** Windows refuses excess committed memory, the request reports failure, owned descendants are reclaimed, and an existing committed index remains readable

#### Scenario: Three sessions request indexing
- **WHEN** three sessions request indexing concurrently, including different Codex homes
- **THEN** at most one kit-owned indexing job runs, all three projects remain observed, each receives service as preceding bounded work completes, and waiting or busy responses respect finite deadlines without requiring the first session to close

#### Scenario: Refresh fails after a usable index exists
- **WHEN** full replacement or incremental refresh fails or is cancelled after starting writes
- **THEN** the last committed generation remains available with an explicit freshness limitation and no partial generation is reported complete

### Requirement: Deliberate scope and honest freshness

The kit SHALL enable debounced automatic incremental CodeGraph refresh and connect-time catch-up for every active indexed project within the same resource/admission limits as explicit indexing. A project SHALL be active while at least one connected supported agent session selects its canonical root, including simultaneous Codex CLI sessions in different projects; observation SHALL NOT depend on periodically issuing a graph query. The kit SHALL preserve observation and eventual automatic refresh for all such projects while expensive work is serialized. A healthy session SHALL remain automatically serviced beyond 600 seconds without a manual sync, client restart or closing another project's session; the finite indexing deadline SHALL bound an individual work episode rather than the useful session lifetime. A worker may retire or be replaced automatically at a verified healthy boundary without losing queued changes. Actual failed work SHALL retain its bounded recovery policy and SHALL NOT enter a crash/restart loop.

After the last session selecting a project closes, the kit SHALL stop its observation and release unneeded project resources within the bounded idle period, without affecting remaining projects or other clients of the same project. It SHALL avoid permanent watchers for inactive projects, scanning the computer for closed projects, a detached upstream daemon outside kit ownership, duplicate full rebuilds and automatic retry loops after failure. Initial indexing and full rebuilds SHALL remain deliberate bounded operations. The kit SHALL provide reusable project ignore setup and preserve approved generated-log exclusions for the locally selected large acceptance repository. Product code and specification documents SHALL remain accessible through the appropriate code or text tools. Query responses SHALL identify canonical root and indexed generation and distinguish complete, pending, stale, failed and unsupported coverage. Watcher existence or elapsed debounce alone SHALL NOT prove freshness. Agents SHALL wait for a successful relevant refresh, use an explicit bounded sync if the watcher cannot catch up, or use current Serena/source for pending scopes; unchanged verified evidence SHALL be reused without redundant sync calls.

#### Scenario: A source edit occurs during an ordinary session
- **WHEN** files change without an explicit index request
- **THEN** changes are coalesced into bounded incremental work, pending results remain explicit, and successful refresh makes the new state queryable without a redundant agent-triggered rebuild

#### Scenario: Several open projects change concurrently
- **WHEN** at least three Codex CLI sessions remain open in distinct indexed projects and owned source probes are added, changed, renamed and deleted in each, including a burst in the first project
- **THEN** each project's index catches up automatically without manual sync, root switching or closing any session, answers identify the correct project, and a busy first project does not starve the others

#### Scenario: A healthy session exceeds the former worker lease
- **WHEN** an indexed project's session remains open for more than 600 seconds and another source change occurs after that interval
- **THEN** the change is observed and refreshed automatically within the bounded scheduling policy without a user-triggered renewal, while actual overlong indexing work still fails within its deadline

#### Scenario: Only one of several clients closes
- **WHEN** one client closes while another client still selects the same project and clients remain in other projects
- **THEN** observation and queries continue for all remaining clients and no shared active project is retired because its first client disconnected

#### Scenario: The last client of a project closes
- **WHEN** a project's last session closes while another project's session remains open
- **THEN** only the inactive project's observation and unneeded resources retire within the bounded idle period, the other project continues updating, and reopening the inactive project performs automatic catch-up

#### Scenario: A project changed while its worker was inactive
- **WHEN** a client next selects that indexed project
- **THEN** bounded catch-up runs before the changed scope is reported current and does not start a second unsupervised indexer

#### Scenario: An automatic refresh reaches its resource limit
- **WHEN** a watcher or catch-up operation fails its memory, time or storage limit
- **THEN** its owned work is reclaimed, the graph is marked stale or failed, and no automatic crash/restart loop consumes more resources

#### Scenario: The affected consumer is indexed
- **WHEN** the locally selected large acceptance repository is indexed with the delivered ignore policy
- **THEN** generated logs stay excluded, all eligible maintained sources are accounted for, and representative product symbols and specification documents remain available through the appropriate code or text tools

#### Scenario: A file is too large or has a different syntax than its extension
- **WHEN** upstream extraction skips an oversized file or misclassifies a project-specific format
- **THEN** coverage identifies that limitation without deleting or rewriting the file, silently broadening exclusions, or declaring a complete semantic index

## ADDED Requirements

### Requirement: Shared project resources across sessions

The kit SHALL reuse the same canonical project's index, observation and suitable backend processes across its clients, including clients from different Codex homes. An additional client SHALL NOT create a duplicate full index, watcher or heavy backend for that project. Different projects SHALL retain isolated source, generations and client response details while sharing bounded admission and suitable process infrastructure. Available published CodeGraph sharing mechanisms and existing native ownership primitives SHALL be reused where compatible with the accepted resource and recovery constraints; an unowned daemon or a new heavy resident process per CLI client SHALL NOT substitute for that integration. Acceptance SHALL measure owned process counts and aggregate memory with one client, additional clients of the same project, and several distinct projects rather than infer resource savings from configuration alone.

#### Scenario: A second session opens the same project
- **WHEN** another Codex CLI session selects an already active canonical project, including through a different Codex home
- **THEN** it reuses that project's index and observation/backend ownership, receives its own correctly scoped responses, and closing either client leaves the other's service intact without a duplicate indexer

#### Scenario: Several projects share the resource allowance
- **WHEN** several distinct indexed projects are simultaneously active
- **THEN** actual owned process and memory measurements establish reuse and the retained aggregate resource limits, while queries and automatic refresh remain usable in each project

### Requirement: Bounded index and response storage

The managed entry point SHALL keep owned indexes, staging generations and retained response details in explicit local locations, check free space before growth, bound retained history, and stop with a recoverable error before exhausting its configured storage budget. Normal queries SHALL NOT rebuild indexes, create repeated full copies or write unbounded logs. Cleanup SHALL remove only verified owned obsolete data and SHALL NOT follow links into unrelated paths. Private source responses and consumer identities SHALL stay outside the reusable repository.

#### Scenario: An index approaches the storage allowance
- **WHEN** database, journal and staging growth reaches the configured limit or free-space reserve
- **THEN** the operation stops, reports the storage cause, preserves the previous usable generation and leaves no unbounded retry running

#### Scenario: A response detail expires
- **WHEN** bounded local retention removes an old response
- **THEN** subsequent access reports expiry explicitly and does not invent omitted content or silently rerun the original query
