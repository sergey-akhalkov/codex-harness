## Context

The installed launcher already opens executors as titled tabs of the lead's
Windows Terminal when `WT_SESSION` is set, without focus stealing, and falls
back to a visible console otherwise. The instruction layer was written for the
older tiled-window design and still says every conversation must be
"simultaneously visible in its own window or pane" and that a "switchable list"
is insufficient. A careful lead model resolves that conflict against tabs and
tries to make separate windows fit one screen, using the only window-management
tools it has (Nuphus desktop operations) on its own terminal. The fix is
instruction and specification alignment, not launcher work.

## Goals / Non-Goals

Goals:

- One rule everywhere: a dedicated, titled terminal surface per conversation
  (tab, pane or window), established by the dispatch command before the first
  model request.
- Explicit permission for same-terminal tabs and explicit prohibition of
  agent-side window resizing, moving and tiling.
- Preserve the anti-hiding guarantees: no hidden model processes, no raw-log or
  single-chat-identity substitutes, no screenshots or pixel polling of agent
  windows, suspend-and-restore on view loss.

Non-Goals:

- Changing the launcher's tab dispatch, receipts or fallback console behavior.
- Removing the visible-console fallback for terminals without `WT_SESSION`.
- Weakening executor isolation, steering, board or recovery contracts.

## Decisions

- **Surface, not simultaneity.** The observable requirement is what a user can
  inspect: each conversation has its own titled surface with assignment, model,
  effort and live activity. Whether surfaces are tiled on screen at the same
  moment is not a requirement; that wording caused the ceremony.
- **Tabs are sufficient.** A Windows Terminal tab per conversation keeps every
  conversation individually inspectable and titled, which satisfies the intent
  the old "switchable list" clause protected (no masked or hidden chats).
- **The dispatch command owns view establishment.** `executor spawn` opens the
  tab and reports it; the lead never pre-arranges the desktop. If the surface
  is missing or lost, the lead reports it and restores through the owning
  command, not through Nuphus window operations.

## Risks / Trade-offs

- A user who wants all conversations tiled on one screen must arrange that
  personally; the kit no longer implies agents will. This matches the reported
  everyday use on a laptop, where the ceremony cost more than it returned.
- Relaxing "simultaneously visible" could be read as permitting hidden chats;
  the rewritten clauses keep hidden processes, raw logs and masked identities
  explicitly insufficient.

## Migration Plan

1. Update both owning specifications' requirement wording through this change.
2. Align the live instruction surfaces (developer instructions, team-lead
   skill, delegation guide) in the same commit.
3. Revise the project decision record to the user's 2026-09-20 correction.
4. No deployment step is required for behavior: the skill and developer
   instructions are live links/overrides; the installed launcher already has
   tab dispatch.
