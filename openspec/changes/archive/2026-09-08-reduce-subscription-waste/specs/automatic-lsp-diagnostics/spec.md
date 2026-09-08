## ADDED Requirements

### Requirement: Conditional automatic diagnostics

Automatic LSP SHALL remain disabled unless the subscription-efficiency benefit gate accepts a specific capability and scope, including its languages and mutation entry points. The following automatic behaviors SHALL apply only to that accepted scope. Neither installation nor completion of implementation SHALL implicitly enable it. An accepted Serena edit-result integration MAY operate with all native hooks still disabled. Other write routes SHALL retain applicable explicit project verification and SHALL NOT be represented as having received automatic diagnostics.

#### Scenario: An implemented backend has not passed the benefit gate
- **WHEN** an ordinary Codex session reads or edits files
- **THEN** that backend does not run automatically

#### Scenario: Only Serena edit-result diagnostics are accepted
- **WHEN** an eligible Serena operation changes supported content and a different operation uses a non-selected write route
- **THEN** only the accepted changed-file operation receives automatic diagnostics, native hooks remain off, and the other route's explicit verification scope is recorded honestly

## MODIFIED Requirements

### Requirement: Automatic diagnostics across edit entry points

Automatic LSP SHALL run ONLY after actual content modification or creation of a supported file through a native patch, shell operation or MCP edit, and SHALL target ONLY the files actually created or modified by that operation. Existing dirty state alone is not a new modification. Rewriting identical bytes, reads, unchanged commands, startup, Stop/SubagentStop, deletion-only operations, unrelated external changes and unfinished earlier checks MUST NOT trigger analysis. A failed command that actually writes supported files SHALL receive the same scoped handling as a successful writer. Renaming SHALL clear obsolete identity without analyzing survivors merely because a file was deleted; a newly created supported destination is eligible as a creation. Unsupported files SHALL never start an LSP backend.

#### Scenario: A native patch introduces an error
- **WHEN** a patch creates one supported file and changes another's content to introduce a known error
- **THEN** automatic requests target those two new revisions only and identify the error with its file and position

#### Scenario: Shell changes are not represented by a simple patch
- **WHEN** a shell command or MCP edit modifies a pre-dirty supported file and creates an untracked supported file, then fails
- **THEN** only those actual new revisions are checked and the original operation failure is preserved

#### Scenario: A read-only tool completes
- **WHEN** a read, no-change command or identical-byte write finishes while prior results are pending, stale or failed
- **THEN** no automatic LSP request, retry or diagnostic context is produced

#### Scenario: Unsupported files and deletion
- **WHEN** an operation changes only unsupported files or deletes files without creating or modifying supported survivors
- **THEN** no automatic LSP request or backend startup occurs

#### Scenario: An unrelated session writes a file
- **WHEN** another session changes a file while this session executes a read
- **THEN** the read does not claim that mutation or launch diagnostics

#### Scenario: An MCP rename updates several files
- **WHEN** an MCP operation renames a source file and modifies references in other supported files
- **THEN** only the created destination and actually modified files are checked; obsolete identity is cleared without scheduling unchanged survivors

### Requirement: Agent-visible bounded delivery

An accepted automatic edit batch SHALL produce at most one bounded summary of new relevant diagnostics or their clearance for its originating operation. Full details SHALL be available separately. The initial model-visible summary SHALL be limited to 1000 characters including metadata and truncation notice; identical findings and clean unrelated files MUST NOT be repeated. Analysis SHALL have a finite budget of at most 30 seconds, with one explicit scoped unverified result on timeout or failure. No later read or completion event SHALL automatically reconcile, retry or block because of that unfinished work. The original edit result and exit status SHALL remain intact.

#### Scenario: Edit yields many diagnostics
- **WHEN** the result exceeds the summary limit
- **THEN** the bounded summary identifies relevant errors, disclosed omissions and the retrievable full scoped report

#### Scenario: A server is still initializing
- **WHEN** the eligible edit times out or its backend is unavailable
- **THEN** the originating operation receives one unverified status and repeated reads or completion do not retry or request continuation

#### Scenario: A file edit completes and the server responds
- **WHEN** an eligible check completes within its budget with new findings or clearance
- **THEN** its minimal summary reaches the originating operation before the next completion response, without duplicate delivery

#### Scenario: The first edit precedes backend readiness
- **WHEN** a proven edit occurs before its retained backend is ready
- **THEN** that edit receives one bounded unverified result; later startup or completion neither absorbs it as verified nor retries it automatically

#### Scenario: A subagent completes with pending diagnostics
- **WHEN** a child completes after an edit whose finite check budget expired
- **THEN** completion performs no diagnostic work or context injection; the earlier edit's unverified status remains truthful and cannot be replaced by another agent's results

### Requirement: Project inputs and dependent diagnostics

Analysis of an eligible changed file SHALL honor its language version, compiler inputs, dependency resolution and encoding without rewriting project configuration. Reading dependency inputs necessary to analyze that file SHALL NOT authorize separate requests or automatic delivery for unchanged files. Configuration changes SHALL invalidate affected cached assumptions without scheduling unchanged source cohorts. A supported configuration file's creation or content change is eligible only for that file. Project-wide verification SHALL remain an explicit project-native check outside automatic LSP.

#### Scenario: A changed declaration breaks an importing file
- **WHEN** only the declaration file was modified
- **THEN** the automatic batch targets only that file; detection of importer regressions belongs to the applicable explicit project checks

#### Scenario: A project configuration changes
- **WHEN** a supported JSON configuration file changes
- **THEN** only its new revision is an automatic target and the change cannot fan out to all project files

### Requirement: Verification through real Codex edits

Every retained automatic capability SHALL be verified against actual Codex patch, shell and MCP mutation paths on owned targets outside the harness: selected entry points require positive delivery evidence; non-selected entry points require zero automatic requests/output and honest explicit-verification coverage. Known error/correction cases, partial failures, pre-dirty and untracked files, stale replies and concurrent roots SHALL be covered. Negative cases SHALL assert zero analysis requests and zero diagnostic output for reads, identical writes, unsupported files, deletion-only, startup, completion and old pending work. Direct helper tests alone SHALL NOT establish integration. No retired language SHALL remain an unconditional automatic-delivery requirement.

#### Scenario: Automatic diagnostics are declared delivered
- **WHEN** a retained capability is declared delivered
- **THEN** native evidence demonstrates both correct mutation delivery and zero-trigger negative cases with preserved product acceptance checks

### Requirement: Pre-edit work shares one finite deadline

If retained mutation detection needs pre-edit observations, that work SHALL be bounded below the native hook timeout and SHALL NOT start LSP analysis. Observation SHALL preserve the earliest relevant state across overlapping operations and avoid long database write locks. An unknown baseline SHALL NOT turn current project contents into an automatic analysis batch.

#### Scenario: A journal writer holds its transaction
- **WHEN** an edit's before-state is incomplete or lock-contended
- **THEN** observation remains bounded and only independently proven creations or content changes are eligible

#### Scenario: Traversal exhausts its budget before finding files
- **WHEN** any bounded target observation expires without a complete before-state
- **THEN** existing observations survive, coverage stays incomplete and current project files do not become an automatic analysis batch

### Requirement: Idempotent completion diagnostics

Stop and SubagentStop SHALL NOT start, retry, reconcile or request automatic LSP diagnostics. They SHALL NOT block completion or inject diagnostic context from current, pending or historical findings. Applicable explicit project checks remain required by the consuming task; silence SHALL NOT be represented as a successful LSP check.

#### Scenario: Both completion handlers observe the same backend failure
- **WHEN** cached native and command completion handlers encounter an existing backend failure
- **THEN** both perform zero LSP work, emit no diagnostic context and request no continuation

#### Scenario: Source changes after a native completion
- **WHEN** source, project configuration or backend registry changes before a companion completion handler runs
- **THEN** completion remains silent and schedules nothing; only a separately proven supported-file mutation may authorize its own scoped check

#### Scenario: Completion clears a previous error
- **WHEN** a correction has established current empty diagnostics before Stop
- **THEN** any clearance is delivered with that eligible edit, while Stop performs no check and emits no diagnostic context

### Requirement: Markdown links into sibling directories

When retained, analysis of a created or content-modified Markdown file SHALL validate explicit local file and fragment links using bounded read-only dependency access, including sibling directories. Unrelated resources, network access and expansion to a whole parent directory SHALL remain prohibited. A change in an unchanged document's linked target SHALL NOT itself trigger automatic analysis of that document. Missing targets SHALL be scoped diagnostics rather than protocol failures.

#### Scenario: Existing document links into a sibling checkout
- **WHEN** the edited Markdown contains an explicit permitted local link
- **THEN** validation accesses only the required dependency and reports its actual link status

#### Scenario: Only the linked heading changes
- **WHEN** the source Markdown document is unchanged
- **THEN** it is not added to an automatic LSP batch

#### Scenario: Linked target is missing or has changed
- **WHEN** an eligible Markdown creation or content edit links to a missing file or heading
- **THEN** its scoped check reports that error and a subsequent qualifying correction clears it; changes only to the linked target cannot trigger a check of the unchanged document

#### Scenario: Server requests an unrelated resource
- **WHEN** a backend requests a resource outside the changed document's permitted explicit local dependencies
- **THEN** the client preserves the resource boundary without network access or recursive surrounding-directory discovery

### Requirement: Bounded discovery with explicit coverage gaps

Mutation discovery SHALL remain bounded and distinct from language analysis. It SHALL NOT classify absent observations as deletion or existing files as newly created. An unchanged large file SHALL NOT cause analysis, context delivery or baseline invalidation. A proven changed supported file exceeding analysis limits SHALL receive one scoped unverified status without retries on unrelated operations.

#### Scenario: Large archived input is unchanged
- **WHEN** only the small supported file changes
- **THEN** it alone is eligible and the archive produces no automatic diagnostic work or feedback

#### Scenario: Large input changes
- **WHEN** a proven changed supported file cannot be analyzed within resource limits
- **THEN** its operation receives a bounded unverified status and later non-mutations remain silent

#### Scenario: Incomplete traversal
- **WHEN** target observation exhausts its finite allowance or cannot read a file
- **THEN** partial observations are preserved, missing observations are not classified as deletions or new files, and no unsupported coverage claim or whole-project fallback follows

### Requirement: Forward recovery without invented history

Missing historical mutation evidence SHALL remain unknown. Establishing forward observation SHALL NOT analyze baseline files or emit diagnostic feedback after reads. Later proven creation or modification SHALL be handled independently, without absorbing a concurrent edit into an unchanged baseline.

#### Scenario: Existing session has no baseline
- **WHEN** it reads files and subsequently makes a proven supported-file edit
- **THEN** the read triggers no analysis and only the later edit is eligible

#### Scenario: Fresh session and parallel pre-edit hooks
- **WHEN** bounded observations overlap before the first proven edit
- **THEN** the earliest relevant before-state is preserved, no analysis starts during observation and later observations do not absorb an outstanding mutation

### Requirement: Non-looping infrastructure delivery and transport ownership

If multiple transports remain necessary, they SHALL share one receipt per originating mutation and file revision so only one analysis and summary is delivered. Backend or transport failure SHALL NOT expand file scope, retry on unrelated events or request automatic continuation. When hooks are suspended, both cached native handlers and command companions SHALL return without analysis or model-visible diagnostics.

#### Scenario: Concurrent native and command handlers
- **WHEN** both receive the same changed revision
- **THEN** at most one eligible analysis and summary occurs, including failure cases

#### Scenario: Already-running session retains old handlers
- **WHEN** global suspension is active
- **THEN** both handlers produce no diagnostic work, fallback scan or continuation

#### Scenario: MCP is absent
- **WHEN** the native diagnostic transport cannot handle an eligible edit in an accepted automatic scope
- **THEN** any retained fallback shares that edit's finite budget and delivery ownership, checks only its proven changed files or reports one scoped unverified result, and never scans on non-mutations

#### Scenario: Alternating failures on repeated Stop
- **WHEN** repeated Stop or SubagentStop encounters alternating baseline, lock, scan or backend failures
- **THEN** no analysis, diagnostic context or continuation occurs and silence does not certify a clean result

#### Scenario: Real defect then correction
- **WHEN** an eligible edit introduces a defect and a subsequent eligible edit corrects it
- **THEN** current finding and clearance reach their respective operations once, with stale or duplicate transport results discarded and no completion loop

## REMOVED Requirements

### Requirement: Bounded diagnostic reconciliation makes progress

**Reason**: Resuming pending work on subsequent hooks conflicts with the user's strict creation/content-modification-only trigger rule.

**Migration**: Retain truthful per-edit results; use a new qualifying mutation or an explicitly requested project check for further analysis. Do not restore automatic backlog or completion reconciliation.
