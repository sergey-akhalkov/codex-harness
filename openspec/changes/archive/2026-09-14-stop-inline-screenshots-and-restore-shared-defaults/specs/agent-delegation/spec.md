## ADDED Requirements

### Requirement: Visibility policy does not capture conversation pixels

The requirement that every active model conversation appear in its own window or pane SHALL remain a native UI or controller obligation. Until that capability is verified, agents SHALL keep execution in the already visible main conversation and MUST NOT start hidden model processes. They MUST NOT satisfy the visibility rule by taking screenshots of Codex or other agent windows, sending those images to a model, or polling window pixels for status. Missing simultaneous views SHALL be reported as an unavailable controller capability, not as a Nuphus visual task.

#### Scenario: A child conversation needs a separate view
- **WHEN** a lead would dispatch a child and simultaneous native views are not established
- **THEN** it reports the missing view, keeps the work in the visible main conversation or waits for the controller, and does not screenshot existing Codex windows

#### Scenario: Window identity is needed for an owned desktop target
- **WHEN** an authorized desktop task needs to select a non-Codex window
- **THEN** list/title/state identify the window, and a screenshot is used only if a visual question remains

