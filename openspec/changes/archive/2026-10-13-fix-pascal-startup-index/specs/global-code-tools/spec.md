## ADDED Requirements

### Requirement: Pascal startup avoids redundant eager source loading

Managed Pascal startup SHALL serve working-source semantic requests without
eagerly loading build copies excluded by Serena merely to initialize a second
workspace symbol index. Serena symbol search, document symbols, bodies, semantic
edits and cross-unit navigation SHALL remain available through the adopted backend.

#### Scenario: Build copies accumulate between sessions
- **WHEN** a Pascal project contains excluded generated source copies and a new managed worker serves a semantic request
- **THEN** startup does not load those copies into a redundant workspace symbol index and returns the requested working-source symbols

#### Scenario: Navigation and editing after startup
- **WHEN** a managed session searches working Pascal units, follows a cross-unit relationship and edits an owned fixture symbol
- **THEN** the expected units and symbol bodies are returned and the edit is confirmed by semantic and disk readback
