# Proposal: sweep-native-build-scratch

## Why

An interrupted explicit native build leaks its process-temp scratch target
directory: `native_build.rs` compiles candidates through `tempfile` prefixes
`hcb-` (build), `hcc-` (consumer Check evidence) and `hca-` (activation
evidence), and those `TempDir` values are only reclaimed when the manager
process unwinds normally. A 2026-09-22 disk snapshot found 18 abandoned
`hcb-*` directories (3.3 GiB) in the user temp root after installs and
updates, and each new interrupted build adds roughly 0.2-0.4 GiB. The leak
accumulates on the system drive with no owner performing cleanup.

## What Changes

- Before compiling a new candidate, the explicit native build lifecycle
  removes its own abandoned scratch directories from the process temp root:
  direct children named with the `hcb-`, `hcc-` or `hca-` prefix whose last
  write is older than a conservative sweep threshold (48 hours).
- The sweep is best-effort and guarded: only ordinary directories are
  removed, reparse points are skipped and never followed, unrelated entries
  are untouched, and in-use or unreadable entries are skipped without failing
  the build.
- Fresh scratch directories stay protected by the age gate, so concurrent
  Check/activation evidence from another explicit management operation is
  preserved.
- `docs/rust-native.md` documents the reclamation behavior and the existing
  guidance for closing retained verification targets (soak runs) so native
  verification state does not accumulate on the system drive.

## Non-Goals

- No generic temp cleanup: files outside the three harness-owned prefixes
  remain governed by the user's temp tooling (for example the
  `windows-disk-reclaim` allowlist).
- No change to the fresh-short-path scratch design, build deadlines, Job
  limits, or the immutable-build reuse flow.
- No new CLI surface or configuration option.
