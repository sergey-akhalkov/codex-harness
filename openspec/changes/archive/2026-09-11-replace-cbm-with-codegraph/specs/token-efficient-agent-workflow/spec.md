## MODIFIED Requirements

### Requirement: Precise semantic retrieval and scoped editing

The delivered Serena and CodeGraph selection SHALL support targeted structure, definition, reference and impact queries with compact results. Consumers MUST verify repository identity and current coverage, reuse a verified unchanged index, and refresh or use fresh source on change or incomplete coverage. Serena SHALL be the starting choice for a known-file symbol, exact references and suitable edits; CodeGraph SHALL be selected for bounded repository discovery and relationships when it avoids several source reads. Literal text, configuration and documents SHALL retain scoped native retrieval. A task SHALL NOT require both code tools or graph setup when one already answers it. `apply_patch` SHALL be the preferred native path for suitable bounded text edits; semantic edits and deterministic generators SHALL remain available when they better preserve correctness.

#### Scenario: Retrieve and edit one symbol
- **WHEN** a task needs one definition and its callers in an indexed project
- **THEN** the workflow retrieves bounded relevant results through the suitable provider, applies a scoped edit, and verifies the changed behavior without whole-file regeneration or redundant retrieval

#### Scenario: Index or language coverage is incomplete
- **WHEN** graph results lack verified freshness or requested source scope
- **THEN** the consumer reports the limit and uses current semantic or direct source evidence without treating absence as proof

#### Scenario: Graph relationships are approximate
- **WHEN** an edge resolves to an ambiguous or unrelated same-name symbol
- **THEN** it remains a candidate until verified, and a refactor or exhaustive-reference claim uses current semantic or source evidence

## ADDED Requirements

### Requirement: Enforced code-tool response budgets

Managed CodeGraph responses SHALL be bounded before entering model context. The initial default SHALL be at most 4 KiB of serialized UTF-8 result data, including metadata; explicit larger requests SHALL remain at most 16 KiB. Search and one-hop relationship requests SHALL default to five results. Broad exploration SHALL require deliberate opt-in with a specific question and at most two requested source files by default; file count SHALL NOT substitute for the response-byte limit. Unbounded whole-file reads, unrestricted repository dumps and repeated exploratory fan-out SHALL NOT be default operations.

Responses SHALL retain project and generation identity, errors, warnings, coverage limits and explicit truncation. Oversized content SHALL produce a useful bounded partial answer or a bounded refusal with a local detail identifier and a narrowing hint. It SHALL NOT silently discard matches, label a partial result exhaustive, or return duplicate full payloads in text and structured content. Retained details and their retrieval SHALL be separately bounded; a detail request SHALL NOT rerun the backend. All number/range limits SHALL be validated at the managed boundary. Startup instructions and tool descriptions SHALL remain compact and match actual availability.

#### Scenario: One-file exploration returns a large payload
- **WHEN** upstream exploration returns more than the default response budget despite `maxFiles=1`
- **THEN** the managed response remains within budget, marks omitted material and offers bounded access or narrowing without silently increasing the limit

#### Scenario: A common name has many references
- **WHEN** a search, reference or impact response exceeds its count or byte allowance
- **THEN** the response explicitly states limited coverage and permits refinement by file or symbol instead of automatic pagination of the whole repository

#### Scenario: Upstream response is malformed or failed
- **WHEN** upstream returns a protocol error, oversized frame, invalid encoding or tool failure
- **THEN** a bounded explicit error reaches the consumer, original diagnostic details stay locally bounded, and no apparently successful empty answer replaces the failure

### Requirement: Measured selection without subscription overclaims

Acceptance SHALL compare the same questions on the pack and a locally selected large real repository: exact-symbol lookup/body, file overview, direct callers, cross-file flow, ambiguous name, absent symbol and high-fan-out retrieval. Current source or semantic evidence SHALL establish the answer oracle. Cold preparation, warm queries, schemas/initialization, follow-up reads, parent/child coordination and omitted-result recovery SHALL be accounted for separately. Measurements SHALL distinguish request bytes, upstream bytes, delivered bytes, elapsed time, tokenizer estimates or measurements, and actual subscription usage. Fewer tool calls SHALL NOT by itself prove lower cost.

Small exact tasks SHALL avoid broad exploration and remain within an initial 8 KiB cumulative retrieval budget unless a stated missing fact warrants expansion. Bounded graph tasks SHALL return enough verified information to answer the question; savings obtained by hiding necessary information SHALL fail acceptance. Compared with the selected raw baseline, the optimized path SHALL reduce unnecessary delivered bytes without losing the oracle answer. No fixed weekly-limit saving or overall speedup SHALL be claimed without direct measurement.

#### Scenario: A compact caller query answers the question
- **WHEN** a bounded graph query returns the needed direct callers with verified locations
- **THEN** the agent reuses that answer and does not also fetch full bodies or call Serena merely to complete a tool checklist

#### Scenario: Additional retrieval is needed
- **WHEN** the initial budget cannot answer a required question
- **THEN** the agent identifies the missing fact, narrows the next request, preserves earlier valid results and reports any remaining uncertainty instead of silently escalating breadth

#### Scenario: Updated instructions are delivered
- **WHEN** a new parent or tool-capable child starts outside the pack
- **THEN** it receives the current selection and budget rules through the existing global links, and already running sessions are identified as needing reload
