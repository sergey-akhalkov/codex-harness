## ADDED Requirements

### Requirement: Desktop inspection without conversation screenshots

Global instructions SHALL prefer Nuphus window list, title, bounds and state, and browser snapshots with element references, for identity and interaction. They SHALL reserve desktop or window screenshots for a genuine visual question about non-text UI. Agents MUST NOT capture Codex conversation windows, the desktop, or other agent views to satisfy simultaneous-visibility policy. Polling screenshots of the same window SHALL NOT be a default progress check.

#### Scenario: Visibility of another conversation is unknown
- **WHEN** simultaneous native views are unfinished and an agent needs to know whether another conversation exists
- **THEN** it reports that the required view is unavailable or uses non-image window identity, and keeps work in the already visible main conversation

#### Scenario: A visual UI question is in scope
- **WHEN** the task needs to read a control, glyph or layout that text/state tools cannot answer
- **THEN** a bounded screenshot of that owned target remains allowed through the visual capture contract

