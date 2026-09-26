## Context

See `proposal.md` for motivation. Current mechanics that shape this design:

- Message content reaches the commands only through argv (`--text`) or a
  caller-created file (`--file`). Windows `CreateProcess` bounds the whole
  command line near 32 KiB, so `--text` fails at the OS/shell layer long
  before the harness bound; callers then hand-write a file.
- Both directions enforce a 256 KiB inline payload bound before sending
  (`MAX_TEXT` in `executor_message.rs`) and refuse above it. The native
  WebSocket control transport accepts records up to 1 MiB
  (`RECORD_LIMIT` in `harness-core/src/task_control.rs`), so 256 KiB is a
  product margin, not the physical ceiling.
- Lead-side commands declare `--source`, `--codex-home`, `--slot`, `--owner`
  and optional `--session` as required flags, then verify them against the
  same dispatch receipts, leases and endpoint records the harness maintains.
  `CODEX_HOME/harness/installation.json` already records the installed
  source checkout.
- The active change `add-executor-lead-messaging` (tasks 5.1-5.4 open) owns
  the reverse channel, reply references, waiting lifecycle and watch exit 3.
  This change is a UX layer over those owners and composes with it.

## Goals / Non-Goals

**Goals:**

- One mental model for message content: pipe it (or pass short `--text`);
  any size up to a fixed ceiling delivers without caller file management.
- Recorded-identity defaults for lead-side executor commands with ambiguity
  refusal and unchanged verification strength.
- Honest delivery evidence for spilled messages in existing receipts/watch.

**Non-Goals:**

- No new transport, protocol, provider or conversation service; native
  `turn/steer` / `turn/start` remain the only delivery path.
- No chunked multi-message delivery, no re-sending tasks, no prompt rituals.
- No changes to installation-lifecycle command explicitness.
- No removal or semantic change of any existing flag.

## Decisions

### D1. Content sourcing: piped stdin is a first-class source

When neither `--text` nor `--file` is present and stdin is not a terminal,
read the message from stdin; `--text -` selects this explicitly. On Windows,
`stdin` pipe detection uses the existing console/pipe discrimination the
shell path already relies on. An empty stream or interactive terminal is
refused as "no content" - never blocking, never an empty send. Stdin must be
valid UTF-8 with the same BOM handling as `--file`.

Alternative considered: a helper subcommand that writes the caller's file.
Rejected: it is exactly the manual ceremony this change removes. Raising the
argv limit is not possible (OS bound).

### D2. Deterministic spill threshold, not reactive transport probing

Keep 256 KiB as the inline bound. Payloads above it are spilled proactively.
The alternative - send inline, catch a transport/provider size refusal, then
spill and retry - makes delivery depend on parsing remote error strings,
risks double delivery through the existing retry-suppression machinery, and
behaves differently per provider. A fixed local threshold keeps one code path
and a testable rule: small messages are byte-identical to today; large ones
always spill.

### D3. Spill file and pointer envelope

Spill files live at `CODEX_HOME/harness/messages/<message-id>.txt`, written
once from the complete in-memory payload before any send. The delivered
envelope is the existing header (sender, run, session, metadata, reply
reference) plus a compact pointer: exact byte length, absolute spill path,
and an explicit instruction that the full message is in that file and must be
read now. The pointer envelope participates in the existing content-identity
and duplicate-suppression logic; a retry after an indeterminate result cannot
deliver the payload twice.

Alternatives: writing into the recipient worktree (pollutes the repo and
commits), `%TEMP%` (uncontrolled lifecycle), or splitting the payload into
multiple ordered conversation inputs (ordering, partial-delivery and
dedup complexity far exceed the pointer's cost).

### D4. Spill ceiling 8 MiB and lifecycle ownership

Stdin and file reads stream against an 8 MiB ceiling; above it, refuse with
the actual number and send nothing. The ceiling prevents runaway reads, not
model context management - the recipient sees the exact size and decides how
to read. Spill files are owned by run-record cleanup: when a run's receipts
and lease records are cleaned, its message files go with them; the messages
directory carries a size bound with an honest degradation error rather than
silent eviction. Receipts record `payloadPath`, `payloadBytes` and
`delivery: "spill"`; watch output shows the same fields. A spilled delivery
never reports as inline delivery.

### D5. Address defaults from recorded identity only

Resolution order: `--codex-home` from the flag, else `CODEX_HOME`, else the
recorded installation default; `--source` from the flag, else the source
checkout in `installation.json`; run identity (slot/owner/session) from the
live lease and receipt set scoped to that home and source. Exactly one live
run resolves automatically; several produce a bounded listing naming slots
and the `--slot` disambiguator; none produces the existing honest state
error. Resolution reads only harness records - never cwd, process names,
window titles or "most recent session" - and the resolved values then pass
through the identical verification used for explicit flags. Explicit flags
always win. `watch`, `stop` and the steering `message` form share this
resolver; `pool` listing keeps its current explicit-only nature where it is
already parameter-free in practice.

### D6. `spawn --exec -` reuses the stdin reader

The same bounded UTF-8 stdin reader serves `--exec -`; the streamed text
enters the existing assignment rendering unchanged. Empty or terminal stdin
refuses before slot allocation.

### D7. Composition with `add-executor-lead-messaging`

That change archives first; its delivery, reply and waiting owners are the
base this change layers onto. Both changes modify the steering and addressed
message requirements, so at archive time this delta must be reconciled with
the then-current main specs (headers match by design; the merged text keeps
both changes' guarantees). Implementation touches the same
`executor_message.rs` / `executor_cli.rs` owners and must not reopen that
change's open acceptance tasks.

## Risks / Trade-offs

- [Recipient model ignores the pointer file] -> The envelope instructs an
  immediate read, watch/receipt surfaces repeat the path, and installed
  acceptance includes a real spilled exchange consumed end to end.
- [Exec tooling always attaches a non-terminal stdin, making "no content
  flag" ambiguous] -> A non-terminal stdin with an empty stream is a refused
  empty message, not a hang; `--text`/`--file` remain unambiguous explicit
  sources.
- [Defaults target a different run than intended] -> Ambiguity always
  refuses with a listing; the same identity verification rejects stale
  bindings exactly as explicit values do today.
- [Spill accumulation] - Files are tied to run-record cleanup and a bounded
  directory; degradation is an honest error, never silent eviction.
- [Instruction growth] - Minimal forms replace path-heavy examples; the
  agent-delegation delta requires the combined load not to grow, verified by
  the existing accounting task in `add-executor-lead-messaging` style.

## Migration Plan

Additive: new optional stdin source, automatic spill above the unchanged
inline bound, and optional address fields. No existing command invocation
changes meaning; worst case a caller who previously received the 256 KiB
refusal now gets automatic spilled delivery. Roll out through the normal
harness build/install lifecycle; rollback is reinstalling the prior
immutable launcher build.
