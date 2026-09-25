## Why

Both messaging directions die on content size in ways the caller model must
repair by hand: `--text` travels through the process command line, so a long
message fails at the shell/OS level, and every payload above the 256 KiB
inline bound is a dead-end error even through `--file`. The caller then
manually writes a file and retries with `--file`, and the lead-side steering
form additionally demands checkout, home, slot, owner and session values that
the harness already records. Delegation messaging should be usable without
knowing any size limit or typing any address the code can derive itself.

## What Changes

- Accept message content on standard input for `executor message` and
  `lead message`: when neither `--text` nor `--file` is given and stdin is
  piped, the full UTF-8 stdin content is the message; `--text -` selects the
  same source explicitly. `--text TEXT` and `--file FILE` keep their current
  literal semantics, so no existing caller breaks.
- Deliver oversized content automatically instead of refusing it: when the
  composed message exceeds the inline delivery bound, the command itself
  persists the complete literal payload into a harness-owned message file and
  delivers a compact pointer envelope (message id, exact size, absolute path)
  into the same native conversation. The recipient model reads that file with
  its ordinary tools; the sender never creates or names a file. Both
  directions and the `--reply-to` reply form use this one mechanism.
- Keep delivery evidence honest: a spilled delivery is recorded with its own
  result class and the spill file's absolute path in the existing message
  receipts and watch output; a local file write is still never presented as
  model delivery.
- Bound the spill itself: payloads above a fixed spill ceiling (8 MiB) get an
  honest refusal naming the actual ceiling; nothing is silently truncated or
  split into unordered fragments.
- Own spill files by the run lifecycle: they live under the recorded
  harness message state directory, are cleaned together with the owning run
  records, and the directory stays size-bounded.
- Default the lead-side operational parameters instead of requiring them:
  `--codex-home` from `CODEX_HOME` or the recorded installation default,
  `--source` from the installation record, and slot/owner/session resolved
  from the recorded pool identity when exactly one live run exists. Multiple
  live runs produce an actionable listing that asks for `--slot`; zero or a
  stale one produces the existing honest state error. Verification against
  recorded identity is unchanged; ambiguity is never resolved by guessing.
- Apply the same stdin source to `executor spawn --exec -` so a long
  free-text assignment no longer depends on command-line length.
- Installation-lifecycle commands (`install`, `update`, `recover`,
  `disconnect`) keep requiring their explicit paths: they mutate global state
  where explicitness is the safety property.
- Update CLI help, generated assignment guidance, `team-lead` skill and the
  delegation docs so the minimal forms are the documented first choice.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `lead-agent-orchestration`: message content may arrive from stdin; oversized
  payloads are delivered through an automatic harness-owned spill file with a
  pointer envelope instead of a refusal; delivery evidence distinguishes
  spilled delivery; lead-side executor addressing gains recorded-identity
  defaults with ambiguity refusal.
- `agent-delegation`: delegation guidance documents the size-free minimal
  command forms (stdin content, automatic spill, defaulted addresses) and the
  commands stay usable from a fresh installed consumer without manual path
  insertion; spawn accepts stdin assignment text.

## Impact

Main implementation owners: `crates/codex-harness/src/executor_message.rs`
(content sourcing, spill envelope, delivery evidence), `executor_cli.rs`
(argument defaults, stdin for `--exec`, usage strings), the existing receipt,
  lease and installation-record readers they already use, and `main.rs` help.
Docs and instruction owners: `.agents/skills/team-lead/SKILL.md`,
`docs/agent-delegation.md`, `docs/rust-native.md`, generated executor briefs
in `executor_assignment.rs`.

No new external service, dependency, provider or conversation protocol; the
native app-server turn/steer and turn/start path remains the only transport.
The 256 KiB inline bound stays; spill changes its failure mode from refusal
to automatic delivery. Explicit `--text`, `--file` and all address flags
remain accepted, and identity verification is not weakened.
