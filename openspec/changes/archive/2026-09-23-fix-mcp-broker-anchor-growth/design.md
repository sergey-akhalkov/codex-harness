# Design

## Root cause

The Serena and CodeGraph MCP entrypoints resolve their shared broker location
through a small JSON anchor (`serena-broker.json`,
`coding-agents-harness-codegraph.json`). Each entry is ~150 bytes and one entry
is appended per delivered build generation because a root that already appears
in the generation list is never reused; only the pre-generation “legacy” root
was reusable, once. After ~28 deliveries both live anchors exceeded the fixed
4096-byte read bound (4216 and 4231 bytes observed), so both MCP processes
exited with code 2 before stdio serving and Codex reported
`connection closed: initialize response`. The 4096-byte bound is a read
robustness guard, not a product limit, and the record has no other cap.

## Approach

Keep the one-location-per-live-generation model and fix both properties in the
shared generation chooser:

1. Reads accept a larger but still fixed bound (64 KiB, `ANCHOR_LIMIT` in
   `broker_state`), shared by the Serena broker, CodeGraph account and
   read-only CodeGraph lookup. The bound stays a guard against unbounded reads,
   not a lifetime limit.
2. `choose_generation` treats a generation as live only when its broker
   instance is actually held (the same instance claim already used for the
   legacy root). Entries whose directory vanished stay dropped; entries whose
   broker is dead are pruned; a new generation reuses a free location (legacy
   root first, then a freed generation root) before preparing a fresh one.
   Live older generations keep their entries and roots exactly as today.
3. Rewrites stay under the existing account-wide admission mutex and the
   existing guarded file replacement, so concurrent startups stay serialized
   and crash recovery is unchanged.

After the fix, a first start on an affected installation parses the grown
record, finds no live old broker, reuses one free location, and rewrites the
anchor to the current entry (plus any live generations). Subsequent deliveries
reuse free locations instead of growing the record and directory set.

## Rejected alternatives

- Raising the byte bound alone restores startup but keeps unbounded record and
  directory growth; the same outage recurs after enough deliveries. Rejected
   as the primary fix.
- Deleting old broker directories during startup would mix retirement into
   session startup and race explicit maintenance retirement; pruning only the
   record entries leaves existing explicit retirement and host-residue policy
   unchanged.

## Test strategy

- Unit regression on the exact failure: a record grown past 4096 bytes with
  many historical generations resolves a location for a new source and is
  rewritten pruned and small, for both the CodeGraph account path and the
   Serena broker path. These fail on the current baseline.
- Generation chooser behavior: a dead root is reused instead of preparing a
  new one, a live older generation is preserved, and the retained record stays
  bounded across successive sources.
- Post-delivery MCP availability through the real entrypoints: the CodeGraph
  published-package check and the Serena adopted-package proxy check start with
  a pre-grown anchor and must complete `initialize` and `tools/list`; the
  unignored unit/chooser checks run in the standard native verification.

## Work ownership

The correction is one shared-code seam (`choose_generation`) consumed by both
broken MCP paths, with tests in the same files. There is no disjoint
file/system slice whose parallel integration would cost less than direct
implementation, so the change is implemented sequentially by the lead; executor
dispatch would duplicate the same root-cause context without parallelizable
ownership.
