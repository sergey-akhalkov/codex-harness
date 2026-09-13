## Why

The adaptive-workflow rules already describe decomposition and session reuse, but a later development episode still used a long acceptance cycle to discover individual driving and observation problems. The missing result is reliable selection of the next useful operation during ongoing work, supported by a usable local feedback path and acceptance close to that failure pattern.

## What Changes

- Make the unresolved fact and the smallest check that preserves its conditions the basis for an expensive repeat. A different error or an additional diagnostic does not by itself justify repeating preparation.
- Distinguish preparation, driving, product action, observation and restoration. Obtain and report the original failure as soon as evidence permits, while tracking restoration separately and preserving its safety obligations.
- Make direct, observed exploration of unfamiliar required interactions an explicit path: reuse a valid owned environment, perform a bounded action, inspect its effect, then choose the next action. Promote understood actions into the existing automation owner and complete unchanged parent acceptance.
- Route slow or inconclusive verification to the existing verification and token-workflow skills. Cover unavailable short checks, stale instructions, interrupted sessions and necessary full integration without adding approval gates, universal retry quotas or a controller.
- Verify source-linked delivery separately from actual decisions, real tool effects and complete task cost. Use an independently driven fresh external native task and retain failures and limits.
- Repair the unsynchronized requirements from the archived adaptive-workflow change in the two affected main specifications, preserving other active changes.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `global-working-principles`: Actionable feedback selection, direct exploration, early failure visibility, valid reuse and representative behavioral acceptance; restore the previously accepted adaptive requirements missing from the main specification.
- `agent-delegation`: Restore the archived workstream ownership, consumption and recovery requirements missing from the main specification.

## Impact

Implementation stays in `global/principles-of-work.md`, `.agents/skills/project-verification/SKILL.md`, `.agents/skills/token-efficient-workflow/SKILL.md` and their existing owning operation/decision records. Global delivery uses current source links. Externally maintained OpenSpec workflows, unrelated active implementations and consumer application code remain outside this change.

Acceptance uses existing native execution and inspection capabilities with owned external inputs. Public artifacts contain general scenarios and scoped conclusions; private paths, application identities, native conversations and raw evidence remain local. This change does not promise perfect instruction compliance, universal speedup, or compatibility with an unexercised application.
