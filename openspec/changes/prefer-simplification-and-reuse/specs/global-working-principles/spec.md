## MODIFIED Requirements

### Requirement: Research precedes substantive design and implementation

The global principles SHALL establish Occam's razor and DRY as mandatory solution-selection principles across code, tools, instructions, documentation, build and verification work. Before substantive design or implementation of a feature or subtask, agents SHALL first examine whether the required outcome can be achieved by simplifying the affected existing solution: removing redundant stages, entities, dependencies, duplicated knowledge or work, or improving its representation. This examination SHALL preserve required behavior, correctness, integrity, reliability, performance and acceptance; it SHALL NOT become unrelated cleanup or a requirement to exhaust all possible refactoring.

Agents SHALL then search for and examine existing project and external solutions, reusable logic, applicable standards, official guidance and established practices. They SHALL consider direct reuse, adaptation and targeted refactoring that enables reuse before creating a new implementation. A missing convenient API or standalone component SHALL NOT establish the absence of a suitable foundation. Extraction into shared methods, modules, libraries or tools SHALL serve confirmed consumers, preserve necessary existing behavior and integrate affected consumers with the shared owner, removing superseded duplication within the authorized scope. Similar-looking code alone SHALL NOT justify a speculative abstraction or forced coupling.

Research SHALL compare credible approaches against the operating scenario, compatibility, resource use, integration effort, trust and ongoing maintenance. Agents SHALL evaluate total complexity and cost across understanding, implementation, checks, runtime, use, recovery and change, rather than line count or dependency count alone. New implementation SHALL be limited to a demonstrated remaining gap after simplification, direct reuse and reuse through adaptation/refactoring have been assessed. Its justification SHALL identify the closest relevant foundations and specific reasons they cannot meet the requirements with proportionate adaptation; familiarity or preference for ownership SHALL NOT suffice.

The decision SHALL explain reuse, adaptation/refactoring or custom implementation with relevant sources and specific gaps or trade-offs in the existing owning planning or decision record. Search rank SHALL NOT substitute for investigation. Agents SHALL reuse current applicable findings and bound further research by unresolved consequential decisions; routine edits SHALL NOT require repeated searches or a new report ritual. Unavailable evidence and the limits of a search SHALL remain explicit, without claiming that no solution exists beyond the inspected scope. Benefits in speed, resource use or defect prevention SHALL remain hypotheses until supported by applicable observations and checks.

#### Scenario: Simplification meets the required outcome
- **WHEN** redundant processing, states or dependencies in the affected solution cause the cost or complexity being addressed
- **THEN** the agent evaluates removing or consolidating them before proposing an additional mechanism, preserving required acceptance and checking the affected behavior

#### Scenario: An established capability fits the scenario
- **WHEN** an existing project, platform or dependency capability satisfies the requirement
- **THEN** the agent verifies its fit and reuses it before designing a replacement, retaining the source and rationale in the owning planning or decision record

#### Scenario: Existing logic needs refactoring to become reusable
- **WHEN** a new consumer needs behavior already implemented inside another component but cannot call it through a suitable interface
- **THEN** the agent assesses targeted adaptation or extraction before duplicating the behavior, and when suitable makes the affected existing and new consumers use the shared implementation while preserving their required behavior

#### Scenario: Several external approaches are plausible
- **WHEN** a feature has unfamiliar design choices or a new reusable dependency is proposed
- **THEN** the agent examines relevant primary sources and compares credible alternatives, including adaptation, integration, trust and maintenance costs, before choosing an approach

#### Scenario: Apparent reuse increases overall complexity
- **WHEN** a candidate requires disproportionate dependencies, coupling or maintenance, or combines behavior that only looks similar
- **THEN** the agent explains the concrete mismatch and evaluates a simpler foundation or bounded remaining implementation rather than forcing reuse or creating a speculative framework

#### Scenario: Custom work is justified
- **WHEN** researched alternatives do not fit the requirements even after proportionate adaptation or refactoring, or have unacceptable integration, maintenance or trust costs
- **THEN** the agent explains the specific mismatch, reuses suitable foundations for the parts they cover and implements only the remaining gap without claiming that no solution exists beyond the inspected scope

#### Scenario: Research is already sufficient or unavailable
- **WHEN** current applicable evidence already supports the decision, or external research is unavailable
- **THEN** the agent reuses sufficient evidence without mechanical repetition, or reports the research limitation and continues independent safe work without inventing findings, treating unavailable search as proof of absence or adopting an unevaluated dependency

#### Scenario: A proposed simplification weakens acceptance
- **WHEN** an apparent resource or speed improvement removes required checks, changes required behavior or transfers unaccepted recurring work to the user
- **THEN** the agent rejects that shortcut and preserves acceptance, distinguishing expected benefits from observed results
