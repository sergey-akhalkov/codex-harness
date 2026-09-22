## Context

The executor pool binds each slot to one owner id while its session host is
live. `executor spawn` is the sole allocator: it claims a slot, fetches the
upstream, resets and cleans the tree, and binds the synchronized base. An
interrupted session must continue as the exact session id in the same checkout
with its partial work intact, which spawn cannot express: its freshness
contract requires the reset, and its child arguments start a new session.

The repeated failure came from bypassing spawn: a hand-edited receipt named a
new task owner, while `reconcile_slots` had already cleared the dead session's
owner from the slot record (a clean tree returns to the pool, a dirty one
becomes awaiting review - both with no owner). `executor run --file` then
refused in `record_lease` forever, because nothing rebinds the slot.

## Goals / Non-Goals

Goals:

- One kit command resumes an interrupted pooled executor on its own slot.
- Preserve partial work: no fetch, reset, clean or base change on resume.
- Keep the one-live-owner-per-checkout invariant and the lease lifecycle.
- Keep resume deterministic: the caller names the exact slot, owner and
  session id; nothing is guessed from rollout contents.

Non-Goals:

- Teaching `executor run --file` to adopt slots: it is a tab host, not an
  allocator; adoption there would keep the hand-edited-receipt workflow that
  caused the failures and bypass base bookkeeping.
- Resume for the interactive `tui` mode or picker-based session selection.
  The pooled executor default is `exec`, and exact-session selection is
  already the kit rule for non-interactive resume.

## Decision

`codex-harness executor resume --source CHECKOUT --codex-home DIRECTORY
--slot N --owner ID --session SESSION_ID [--profile ID]
[--terminal-profile NAME] [--terminal-window NAME] --exec PROMPT`

- The session id is validated by the existing exact-session rule (no `--last`,
  picker, whitespace or leading dash).
- Reconciliation runs first. Adoption then requires: a slot record for the
  named index belonging to the same source checkout, an existing slot
  directory, a recorded synchronized base, no live owner elsewhere, and either
  no recorded owner (the interrupted session was reconciled) or the same
  owner. Another owner's claim is refused with both ids named.
- Adoption rewrites the record to `occupied` with the requested owner and
  keeps the recorded base; the tree is untouched. The existing lease recording
  then verifies owner equality and the existing watcher, receipt and terminal
  hosts run unchanged.
- The explicit `--slot` is deliberate: after reconciliation the owner no
  longer identifies the slot, and the lead knows the index from its dispatch
  summary or `executor pool`. Guessing by rollout contents would couple the
  pool to the rollout format for no operational gain.
- `record_lease`'s refusal for an unowned record now names the remedy
  (`executor spawn`, or `executor resume --slot N --owner O --session ID`)
  so a future hand-edited receipt fails loudly with a next action.

## Risks / Trade-offs

- Adoption after reconciliation trusts the caller's slot choice. This is the
  same trust as the lead's release decision; resume never destroys tree state,
  and a wrong slot fails visibly at the session level without data loss.
- Not resynchronizing means the resumed session keeps working from the
  recorded base plus its own commits; that is exactly the interrupted state
  being continued, and freshness returns with the next spawn after release.
