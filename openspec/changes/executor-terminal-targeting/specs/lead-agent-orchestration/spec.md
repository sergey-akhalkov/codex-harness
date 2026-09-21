## ADDED Requirements

### Requirement: Deterministic executor terminal targeting

When the lead runs inside Windows Terminal, executor dispatch SHALL open the
executor tab in the lead's own terminal window, and MUST NOT address the
terminal's most-recently-used or focused window. Dispatch SHALL identify the
lead's window through the console parenting exposed to the calling process,
hold it foreground only as long as the terminal needs to resolve the tab there,
and restore the user's previous foreground window and selected tab once the
titled tab is observably open; the user's deliberate window switches SHALL be
left untouched. When the lead's window cannot be addressed (another virtual
desktop or a blocked activation), dispatch SHALL target the stable
per-checkout window name derived from the source checkout, which the terminal
creates on first use, and MUST NOT create the tab in an unrelated focused
window; an explicit window name requested by configuration or arguments SHALL
be targeted exactly. Activation escalation MUST NOT send synthetic input into
any conversation surface.

#### Scenario: Another project holds the user's focus
- **WHEN** the lead running in one terminal window dispatches an executor while the user works in another window
- **THEN** the executor tab opens in the lead's window, no new terminal window is created, the user's window keeps its foreground position and its selected tab

#### Scenario: The lead window cannot be addressed
- **WHEN** the lead's window is on another virtual desktop or its activation is blocked
- **THEN** the tab opens in the per-checkout harness window instead of the user's focused window, and the user's foreground window is still restored

#### Scenario: An explicit window name is requested
- **WHEN** dispatch receives an explicit terminal window name
- **THEN** the tab opens in exactly that window, and the terminal creates it first when it does not exist

#### Scenario: The terminal activates late
- **WHEN** the terminal summons the receiving window after the launcher exits or activates it again for the created tab
- **THEN** dispatch keeps undoing those activations until they go quiet and leaves the user in their previous foreground window
