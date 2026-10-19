## Why

The pack already requires research and favors simple solutions, but its general instructions do not explicitly require checking whether simplification or refactoring can make existing logic reusable before creating a new implementation. This leaves room to duplicate behavior when a suitable foundation exists but lacks a convenient interface.

## What Changes

- Make Occam's razor and DRY the required order of solution selection: audit simplification, investigate existing foundations, reuse directly or through adaptation/refactoring, and implement only the remaining justified gap.
- Cover project and external code, tools, established approaches, instructions, documentation, build and verification workflows.
- Require actual consumption of extracted shared logic by affected consumers, preserving required behavior and removing superseded duplication within scope.
- Bound research by consequential uncertainty and evaluate total lifecycle complexity, compatibility, trust and maintenance; preserve acceptance and avoid speculative abstractions.
- Keep the canonical policy in the existing portable principles, reconcile its owning specification and decision record, and verify global instruction delivery through the existing live connection.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `global-working-principles`: Strengthen research and solution selection to require simplification and reuse, including reuse enabled by targeted refactoring, before custom implementation.

## Impact

Instruction and documentation changes in `global/principles-of-work.md`, `openspec/specs/global-working-principles/spec.md` and `docs/project-decisions.md`. Existing skills inherit the general policy; specialized self-improvement acceptance remains intact. No new runtime component, dependency, skill, report format or external workflow customization is needed. Verification covers the specification, source hygiene, links, instruction size and model-free loading outside the checkout; it does not claim measured performance gains or universal agent compliance.
