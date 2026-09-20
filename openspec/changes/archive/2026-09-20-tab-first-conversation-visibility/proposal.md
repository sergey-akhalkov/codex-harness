## Why

Delivered delegation instructions still demand that every active conversation be
"simultaneously visible in its own window or pane" and call a "switchable list"
insufficient. Lead sessions read that literally, avoid the launcher's terminal
tabs and perform desktop ceremony - resizing and moving their own terminal with
Nuphus so separate session windows fit a laptop screen - even though
`executor spawn` already opens a titled tab in the lead's own Windows Terminal.

## What Changes

- Visibility is satisfied by a dedicated, titled terminal surface per
  conversation: a tab in the same terminal, a pane or a window. Simultaneous
  on-screen tiling is no longer required.
- The dispatch command remains the owner that establishes the visible surface
  before the first model request; agents do not prepare views manually.
- Agents are explicitly forbidden from resizing, moving or arranging desktop
  windows - including the terminal they run in - to make conversations fit the
  screen; that is not part of dispatch.
- The original anti-hiding intent is preserved: hidden model processes, raw
  logs and one chat identity masking several conversations remain insufficient,
  and the no-screenshot/no-pixel-polling policy is unchanged.
- Instruction surfaces are aligned: `global/harness.config.toml` developer
  instructions, the `team-lead` skill, `docs/agent-delegation.md`, both owning
  specifications and the project decision record.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `lead-agent-orchestration`: the "Isolated and visible executor sessions"
  requirement accepts a titled terminal tab as the executor's visible surface,
  drops the simultaneous-separate-window wording and forbids desktop window
  rearrangement as a visibility mechanism.
- `agent-delegation`: the "Visibility policy does not capture conversation
  pixels" requirement states the tab/pane/window surface rule, keeps pixel
  capture prohibited and adds that missing views are resolved by the owning
  dispatch command, never by agent-side window management.
- `subscription-model-routing`: the "Exact model and effort assignment"
  requirement replaces its simultaneous-window clause with the titled terminal
  surface rule.
- Smaller consistency edits in the `agent-delegation` lifecycle scenario and
  the `lead-agent-orchestration` delivery requirement replace "simultaneous
  conversation views"/"simultaneous visible windows" with titled terminal
  surfaces.

## Impact

- `global/harness.config.toml` (live developer instructions; no rebuild needed).
- `.agents/skills/team-lead/SKILL.md` (live-linked installed skill).
- `docs/agent-delegation.md`, `docs/project-decisions.md`.
- `openspec/specs/lead-agent-orchestration/spec.md`,
  `openspec/specs/agent-delegation/spec.md`,
  `openspec/specs/subscription-model-routing/spec.md` (applied on archive).
- No executable code, launcher or dependency changes; the installed
  `executor spawn` tab behavior is already current and stays as is.
