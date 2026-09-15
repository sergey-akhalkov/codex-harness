## ADDED Requirements

### Requirement: Bounded principles instruction floor

The portable principles document SHALL remain at most 24 KiB while preserving
every normative rule of this specification, including session-lifecycle economy
guidance and the current managed-tool selection. Compression SHALL NOT drop,
weaken or redefine a requirement, and references to retired capabilities SHALL
be removed. The document SHALL remain the single authoritative home of the
principles.

#### Scenario: Size and content after compression
- **WHEN** the compressed principles document is measured and mapped against this specification
- **THEN** its size is at most 24 KiB and every requirement below is still expressible from its text

#### Scenario: Retired capability reference
- **WHEN** a capability leaves the managed selection
- **THEN** the principles document stops naming it as a live selection without losing the general rule it illustrated
