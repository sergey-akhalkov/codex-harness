# Design: sweep-native-build-scratch

## Context

`native_build.rs` creates three owned scratch shapes under the process temp
root: `hcb-*` release-build targets, `hcc-*` consumer-Check evidence and
`hca-*` activation evidence. Normal unwinding deletes them through
`TempDir::drop`; process termination (timeout kill, console close, crash)
leaks the whole tree. The same module already owns state hygiene
(`owner_root`, published-build pruning), so it is the right owner for
reclaiming its own abandoned scratch.

## Decisions

- **Sweep site**: run once inside `prepare()` immediately after acquiring the
  owned-state lock, before `find_reusable`. Explicit install/update is the
  operation that previously leaked; read-only Check and ordinary launches
  keep their no-mutation contract. The sweep runs even when a verified build
  is later reused, because reclaiming abandoned scratch is independent of
  compilation.
- **Identification**: only direct children of `std::env::temp_dir()` whose
  file name begins with `hcb-`, `hcc-` or `hca-`. No recursive search and no
  arbitrary roots.
- **Age gate**: last-write time older than 48 hours. This matches the
  conservative temp age used by the kit's disk-reclaim helper, exceeds the
  30-minute build deadline and the 15-second activation deadline by orders of
  magnitude, and protects scratch created by a concurrent explicit management
  operation even though `prepare()` serializes builds through the owned-state
  lock.
- **Guards**: `fs::symlink_metadata` must report an ordinary directory; any
  reparse point is skipped, and `fs::remove_dir_all` is only invoked on the
  verified prefixed entry. Rust's `remove_dir_all` does not follow symlinks
  or junctions inside the tree, so a junction planted inside abandoned
  scratch cannot redirect deletion.
- **Failure policy**: per-entry best-effort. Removal errors (sharing
  violation, permission, race) are ignored; the sweep never fails or delays
  the build beyond directory enumeration.
- **Verification-state hygiene**: `docs/rust-native.md` gains one sentence on
  stale-scratch reclamation next to the existing fresh-temp-target rule and
  one sentence naming the existing practice for closing retained soak
  targets, without creating a second guide.

## Risks / Trade-offs

- A 48-hour window means a leak persists until the next explicit build after
  the gate; accepted because weekly Storage Sense and the disk-reclaim
  allowlist already bound the interim growth.
- Directory last-write times can be refreshed by external temp cleanup; the
  sweep stays conservative rather than deleting recently written entries.
