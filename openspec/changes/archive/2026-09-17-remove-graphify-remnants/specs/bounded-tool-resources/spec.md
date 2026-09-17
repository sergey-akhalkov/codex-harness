## MODIFIED Requirements

### Requirement: Safe reuse across MCP integrations

Serena SHALL reuse a backend for compatible clients of the same canonical project while routing project activation per client so it cannot switch another client's active project. Nuphus SHALL preserve isolated lazy browser state and element-reference ownership unless shared execution provides equivalent isolation; incompatible state SHALL remain separate rather than being silently merged. All retained tool processes SHALL have bounded idle retirement. Ordinary startup SHALL avoid unnecessary resident shell wrappers without changing tool schemas, STDIO cleanliness or required operations.

#### Scenario: Serena clients choose different projects
- **WHEN** one client activates another project
- **THEN** its subsequent calls reach that project's backend and other clients retain their selected project

#### Scenario: Browser clients coexist
- **WHEN** two clients inspect separate owned browser targets
- **THEN** one client's tabs or element references cannot cause an action in the other client's target, and unused browser engines are not started
