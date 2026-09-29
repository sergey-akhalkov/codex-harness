## MODIFIED Requirements

### Requirement: Per-model default reasoning effort

The launcher SHALL preserve the model and effort selected by effective native configuration, including applicable trusted-project settings, user settings, profiles and explicit invocation overrides. A harness per-model default SHALL apply only when no applicable explicit effort exists. The existing mapping remains a fallback: max for zai/glm-5.3 and xhigh for the supported Grok and Astra mappings; an unmapped model receives no injected effort. A profile, remote route or harness effort selector SHALL retain its documented precedence and bypass behavior. The launcher MUST NOT promote a fallback to a CLI override that defeats an applicable native preference. Effective model, effort and their configuration sources SHALL be inspectable without exposing credentials or changing configuration. Failure to resolve optional harness configuration SHALL preserve the native launch-availability contract.

#### Scenario: Grok starts without an explicit effort
- **WHEN** a session selects a supported Grok mapping and neither its applicable native configuration nor invocation selects an effort
- **THEN** the existing xhigh fallback applies and native session evidence identifies it as a fallback

#### Scenario: GLM keeps its heavier default
- **WHEN** zai/glm-5.3 is selected without an applicable explicit effort
- **THEN** its existing max fallback applies

#### Scenario: An explicit effort wins
- **WHEN** applicable user or trusted-project configuration selects low, or invocation selects another supported effort
- **THEN** the effective native choice survives ordinary and exec launch and no stronger harness fallback replaces it

#### Scenario: Profile and remote behavior is preserved
- **WHEN** a profile, remote route or compatibility effort selector is used
- **THEN** the documented native precedence and existing bypass remain effective

#### Scenario: Unmapped model
- **WHEN** the selected model has no mapping
- **THEN** native effort configuration passes through unchanged

#### Scenario: Untrusted project or unreadable optional defaults
- **WHEN** a project setting is excluded by native trust rules or optional shared configuration is unreadable
- **THEN** the harness does not treat the excluded value as effective, invent a preference, block an otherwise valid native launch or start the payload twice

## ADDED Requirements

### Requirement: Transactional observation publication and recovery

Successfully published RTK handles SHALL identify complete, digest-verifiable observations across concurrent processes sharing one observation store. Creation, publication and retention SHALL have a coherent order; unpublished work MUST NOT be mistaken for abandoned content by another writer. Publication and recall SHALL preserve the documented retention limits, stream identity and no-rerun contract. Serialization SHALL cover store mutation rather than the observed command's execution. Interrupted publication SHALL preserve previously committed observations and leave a recoverable store. A storage failure SHALL preserve usable original command output, its exit status and explicit recovery limitations instead of issuing an unusable handle or rerunning the command.

#### Scenario: Two processes publish below the retention limits
- **WHEN** two independent processes interleave publication at controlled boundaries and the combined store remains below its limits
- **THEN** both successful handles recall their exact originals with valid digests and neither writer removes the other's unpublished content

#### Scenario: Publication is interrupted
- **WHEN** a writer exits before or after the publication boundary
- **THEN** committed observations remain readable, incomplete state is identifiable, and the next operation recovers without deleting another live writer's work

#### Scenario: Recall overlaps retention
- **WHEN** a reader overlaps publication or legitimate eviction
- **THEN** it receives verified content from one committed observation or an explicit expiry/error, never mixed content, a partial successful response or a deadlock

#### Scenario: The store cannot be acquired or written
- **WHEN** bounded store access or publication fails
- **THEN** command output and child exit status survive, the failure is visible, and no successful handle is advertised for missing content

### Requirement: Validated observation metadata confines retention

RTK SHALL validate the supported index schema, observation identity, sizes and digest metadata before using an index to serve or delete files. Invalid records MUST NOT authorize path traversal, arithmetic overflow, deletion outside the owned store or silent destructive reconstruction. Unknown-schema and corrupt-index states SHALL preserve retained evidence and report unavailable recovery explicitly. Existing ordinary expiry remains allowed within the declared retention contract.

#### Scenario: Corrupted handle names an outside file
- **WHEN** an index contains a parent-relative, absolute, separator-containing or otherwise invalid handle
- **THEN** recall and retention reject that record before file access or deletion and an outside sentinel remains unchanged

#### Scenario: Index version or accounting is invalid
- **WHEN** the schema is unsupported, a digest is malformed or retained-size arithmetic cannot be represented
- **THEN** the store reports invalid metadata, does not claim successful recall and does not purge evidence based on the invalid index

### Requirement: Compression selection accounts for the delivered result

An invocation reported as compressed SHALL deliver fewer bytes than its captured original across the complete accounted presentation: both child streams, compact bodies, recovery locators, handle and digest text, exit notices and adapter progress/status/error text. The decision SHALL be made before committing the candidate presentation, using the same accounting basis as diagnostics. All supported compact routes SHALL obey this rule. If the full candidate cannot satisfy it, the adapter SHALL preserve raw output without labeling the invocation compressed. Necessary failure or progress messages MUST NOT be hidden to satisfy the byte comparison; raw, bypass and fault paths make no unconditional no-growth promise. Bytes MUST NOT be represented as exact model-token or subscription savings.

#### Scenario: Metadata outweighs the removed lines
- **WHEN** almost incompressible output, long paths or two stream footers make the complete compact candidate at least as large as the original
- **THEN** raw presentation is selected and diagnostics do not report compression applied

#### Scenario: Progress and a failing child add overhead
- **WHEN** progress was emitted or a nonzero child result requires a status message
- **THEN** all attributable emitted bytes participate in the compression decision and measurement, while the child status and necessary messages remain visible

#### Scenario: A non-Cargo compact route is selected
- **WHEN** an eligible supported filter outside the dual-stream Cargo route prepares compressed output
- **THEN** the same full-presentation rule includes its raw locator and optional observation metadata

### Requirement: Conservative Cargo status recognition

The adapter SHALL remove only lines matching its documented Cargo status layout and recognized status vocabulary. Nonmatching diagnostic and user-output lines SHALL remain byte-preserved. Matching layout alone SHALL NOT be advertised as proof of semantic irrelevance; exact originals and explicit raw operation remain available. The adapter SHALL preserve stream identity, binary/machine-output bypass, nonzero exit status and exactly one child execution.

#### Scenario: A status word has the wrong indentation
- **WHEN** user output contains a recognized status word with spacing that does not match Cargo's documented status field
- **THEN** the line remains in the delivered output

#### Scenario: User output imitates Cargo layout
- **WHEN** arbitrary text happens to match the accepted status layout
- **THEN** documented filtering limitations and exact original recovery remain available and no claim of universally lossless semantic filtering is made

### Requirement: Evidence-gated context optimization through existing owners

Changes to instruction layout, stable prompt sections, deferred tool definitions or deterministic result aggregation SHALL be selected through the existing outcome and skill evaluation contracts. Comparisons SHALL observe the presentation actually delivered to the model, required tool discovery, failures, recovery access and task acceptance. Stable instructions SHALL retain their authority and relative priority. Deferred definitions SHALL remain discoverable when needed; failed discovery MUST NOT look like an empty successful catalogue. Deterministic aggregation SHALL preserve errors, coverage and detail access. Neither shorter source files nor foreign benchmark percentages SHALL establish local savings. Blind head/tail pruning or a runtime migration SHALL NOT become the default without satisfying the declared quality and benefit acceptance.

#### Scenario: Deterministic aggregation removes model work
- **WHEN** a measured recurring operation consists of sorting, joining known identifiers or aggregating structured results
- **THEN** its existing native or Code Mode owner returns the necessary result with errors and detail access, and acceptance compares the complete task rather than only the intermediate payload

#### Scenario: Deferred discovery misses a required tool
- **WHEN** a candidate uses fewer initial definition bytes but fails to discover a required capability
- **THEN** the candidate fails acceptance and the previous supported tool surface remains selected

#### Scenario: Reordering a prefix changes its authority or cache behavior
- **WHEN** a candidate moves stable and changing instructions
- **THEN** actual consumption, priority, correctness and available cache usage are checked on comparable cases before adoption

#### Scenario: An optional candidate remains unproven
- **WHEN** the declared comparison is inconclusive or the local consumer lacks a supported mechanism
- **THEN** the prior default stays active, the evidence and limitation are recorded, and neither deployment nor savings is claimed
