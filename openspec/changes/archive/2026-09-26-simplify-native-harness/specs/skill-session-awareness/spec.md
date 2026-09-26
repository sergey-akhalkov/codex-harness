## MODIFIED Requirements

### Requirement: Derived scoped skill catalogue

The system SHALL derive its view of active skills from the native consumer's effective discovery for the current repository and user. It SHALL identify each skill's name, applicability, canonical path and revision, preserve explicit disablement, distinguish conflicting sources from duplicate links to one source, and exclude candidates, retired packages and foreign project-only skills. Native selection SHALL remain authoritative for what that consumer can use; kit-specific identity or coverage metadata SHALL NOT create a second maintained catalogue or claim to inject instructions into the current model turn. Missing native or identity evidence SHALL be reported explicitly rather than replaced with a complete-looking independent scan.

The view SHALL be regenerable without a second manually maintained registry. Partial discovery or context-budget truncation SHALL be explicit and retain a bounded retrieval route to omitted entries; it MUST NOT claim the complete active set is present. A file-change notification or successful reload SHALL NOT alone establish current-turn awareness, retirement or post-compaction delivery; the existing awareness requirements remain applicable.

#### Scenario: Same source appears through local and global links
- **WHEN** discovery reaches a managed skill by two paths to the same canonical source
- **THEN** the view identifies one source revision without creating competing maintained copies

#### Scenario: Different sources declare the same name
- **WHEN** an active project skill and a different global source have the same name
- **THEN** the conflict is reported with distinct source identities, native effective selection is identified when available, and neither source is silently assumed to override the other

#### Scenario: Catalogue exceeds the context allowance
- **WHEN** the active set cannot fit the compact delivery budget
- **THEN** the session receives an explicit incomplete-list indication and a usable route to discover the remaining relevant entries without loading every skill body

#### Scenario: Native reload does not update a running turn
- **WHEN** native discovery observes a changed skill but its current-turn delivery has not been established
- **THEN** the result distinguishes discovery and revision from model awareness and preserves the supported live-read or continuation route without a false delivery-complete claim
