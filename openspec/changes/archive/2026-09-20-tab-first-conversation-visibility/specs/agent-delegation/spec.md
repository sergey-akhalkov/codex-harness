## MODIFIED Requirements

### Requirement: Visibility policy does not capture conversation pixels

The requirement that every active model conversation appear on its own visible terminal surface - a dedicated titled tab, a pane or a window - SHALL remain a native UI or controller obligation, established by the owning dispatch command before the first model request. A titled terminal tab per conversation SHALL satisfy the requirement; simultaneous on-screen tiling SHALL NOT be required. Until that capability is available, agents SHALL keep execution in the already visible main conversation and MUST NOT start hidden model processes. They MUST NOT satisfy the visibility rule by taking screenshots of Codex or other agent windows, sending those images to a model, or polling window pixels for status, and they MUST NOT resize, move or arrange desktop windows - including the terminal they run in - to make conversations fit the screen. Missing views SHALL be reported as an unavailable controller capability and restored through the owning dispatch command, not as a Nuphus visual or window-management task.

#### Scenario: A child conversation needs a separate view
- **WHEN** a lead would dispatch a child and no titled terminal surface for it is established by the dispatch command
- **THEN** it reports the missing view, keeps the work in the visible main conversation or waits for the controller, and neither screenshots existing Codex windows nor rearranges desktop windows

#### Scenario: Window identity is needed for an owned desktop target
- **WHEN** an authorized desktop task needs to select a non-Codex window
- **THEN** list/title/state identify the window, and a screenshot is used only if a visual question remains

### Requirement: Global lifecycle and recoverability

Sources and setup SHALL activate through the existing linked global lifecycle for new Codex sessions in other projects. Available external models SHALL follow their subscription profile connections; direct native GPT assignment SHALL remain usable when external integration is disconnected. Dispatch SHALL NOT require redundant supplied agent presets. Existing credentials, unrelated user agents and skills, work, MCP integrations and active control channels SHALL be preserved. Updates and disconnection SHALL be verifiable in owned state without stopping a proxy serving other sessions.

#### Scenario: A new external session starts
- **WHEN** the kit is installed and ordinary Codex starts outside this repository
- **THEN** the delegation policy, the `team-lead` skill, orchestration role configuration, board availability, explicit model/effort selection, per-conversation titled terminal views and task recovery are usable without copying sources or repeatedly performing manual setup

#### Scenario: Subscription integration is disconnected
- **WHEN** its lifecycle is exercised in an isolated installation
- **THEN** removed routes are unavailable for new assignments, native GPT remains usable, recoverable task state is retained, and unrelated services and credentials are preserved
