# Design: committed-dispatch-snapshot

## Decision

The synchronized base, not prompt-side file transfer, is the only handoff of
tracked source state to an executor. The lead fixes that base as a commit in
the source checkout before dispatch; dispatch already fetches upstream,
resets the chosen slot to the named revision and verifies a clean HEAD before
the first model request.

## Mechanism mapping

- `--base REV` already overrides the fetched upstream default branch, and
  pool slots are linked worktrees of the source checkout, so a local
  snapshot commit is resolvable without pushing.
- Redispatching with the same `--owner ID` rebinds and resynchronizes the
  same slot, which is the sanctioned path for changed inputs after launch.
- Executor-side duty stays verification-only: a HEAD mismatch is reported,
  not repaired, because the dispatch command owns slot reset.

## Instruction homes

- The `team-lead` skill owns the dispatch workflow rule.
- `docs/agent-delegation.md` owns the freshness contract and mechanics.
- The portable principles carry one general lead-handoff sentence for
  projects outside the kit skill.

## Verification

Strict OpenSpec validation, a grep audit that no text presents post-launch
file copying as synchronization, local-link resolution in edited documents,
and public-source hygiene on the edited files.
