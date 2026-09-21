## Context

The adapter (`tools/rtk-adapter/src/main.rs`) is a single-file Rust binary with four entry points: `hook` (PreToolUse rewrite of a literal `harness-rtk.exe exec <tail>` to `compact`), `exec` (raw passthrough), `compact` (allowlisted compression through pinned `rtk.exe pipe --filter`) and `filter`. `save_raw` already archives retained stdout to `CODEX_HOME/harness/rtk/raw/` with a 32-file keep window and appends a `[rtk raw: <path>]` footer. Failure paths (unsupported form, filter failure, oversize, `HARNESS_RTK_DISABLE`, terminal stdout) intentionally stay raw or compact without retention. The hook definition, trust flow, pinned `rtk.exe` 0.48.0 and the `--token-workflow-only` installation component are unchanged constraints. The mechanism is inspired by the public NVlabs/SoL-Pi ObservationPack design (MIT); no SoL-Pi or Pi code, runtime or dependency is introduced.

## Goals / Non-Goals

**Goals:**

- Grow the accepted adapter, not a parallel subsystem: same binary, same trust, same lifecycle.
- Stable handles with digest-verified, line-windowed recall that works across sessions and fails closed.
- Keep every existing path byte-compatible: compression, bypasses, footer raw locator, exit statuses.

**Non-Goals:**

- Action Fusion (`--then` chains), evidence-preserving reduction, remote or model-backed reduction, PreCompact behavior — separate future changes.
- Automatic interception via PostToolUse output replacement (unsupported by the installed CLI) or any MCP/server surface.
- Per-session archive scoping; deduplication of identical outputs; changes to `raw/` retention or `rtk.exe` behavior.

## Decisions

### Pack storage lives beside `raw/`, as its own bounded area

`CODEX_HOME/harness/rtk/pack/` stores one `<handle>.log` per packed observation plus an `index.json` mapping handle to provenance (source command, SHA-256 digest, byte and line counts, creation time). Packing duplicates the retained bytes that today already go to `raw/`, because `raw/` prunes at 32 files and must keep serving the existing `rtk raw` footer contract. Pack retention is separate: oldest-first eviction at 64 entries or 128 MiB total, whichever is hit first; eviction also removes orphaned pack files without index entries. Alternatives: indexing `raw/` files in place (breaks handles at raw pruning); SQLite index (new non-Rust-friendly dependency for a map a JSON file already serves); content-hash handles (make eviction order and provenance ambiguous).

### Packing engages exactly where retention already succeeds

A handle is minted only on the compressed path where `save_raw` currently succeeds: allowlisted command, filter success, smaller compressed result, output under the 4 MiB limit, non-terminal stdout, no disable. The adapter copies the retained bytes into `pack/` before emitting the footer. If any pack step fails, the run falls back to today's exact behavior (raw locator footer, no handle) so packing can never break compression. Bypass paths emit no handle, matching the spec scenarios.

### Footer keeps the old locator and adds one pack line

The compressed footer keeps `[rtk raw: <path>]` unchanged and appends a single line naming the handle, short digest and the recall command with an example window argument. One extra footer line (~150 bytes) per compressed run is charged to the acceptance measurement rather than hidden.

### Recall is a new subcommand with fail-closed semantics

`harness-rtk.exe recall <handle> [--offset N] [--limit M]` uses 1-based line coordinates, defaults `--offset 1 --limit 200`, clamps `--limit` to 2000 and clamps emitted bytes at 256 KiB with an explicit truncation marker. Output is a compact provenance header (handle, source command, digest verification status, totals, window), numbered lines, and a one-line next-window hint so paging is self-describing. Recall never reruns the source command and never starts models or network calls. Exit codes: 0 success, 2 unknown or evicted handle (with rerun/raw remedy), 3 digest mismatch (integrity failure, no content), 1 usage or I/O error. Alternatives: byte-offset windows (agents reason about logs in lines); relying on `rtk raw` whole-file replay (the exact replay cost this change removes).

### SHA-256 through the pure-Rust `sha2` crate

The digest comes from RustCrypto `sha2` (pure Rust, no build scripts), added only to `tools/rtk-adapter/Cargo.toml`. This keeps the first-party executable boundary Rust-only and avoids launching `rtk.exe` for hashing, whose contract does not guarantee a digest subcommand.

### Concurrency and integrity handling stays small

Handles embed creation nanoseconds plus a monotonic process sequence and files are created with `create_new`, so concurrent sessions cannot collide. The index is rewritten atomically (temporary file plus rename). A lost concurrent update can surface as an explicit unknown-handle error with the rerun remedy, which is honest and recoverable; recall verifies content against the digest before emitting anything.

### Adoption evidence follows the existing benefit gate

The acceptance suite extends `crates/codex-harness/tests/rtk_adapter.rs`: handle and digest emission, no-handle bypasses, exact-window recall with provenance, clamping, eviction errors, integrity failure, session-end survival and byte accounting for footer overhead versus whole-file raw re-read. Results are recorded as measured bytes and avoided reruns only.

## Risks / Trade-offs

- [Disk growth from duplicate retained bytes] → bounded pack retention (64 entries / 128 MiB) with oldest-first eviction and observable not-found errors; same local trust boundary as the existing raw archive.
- [Longer retention of sensitive output] → documented retention, local-only storage, `HARNESS_RTK_DISABLE` bypass unchanged; no remote or model flow exists in this change.
- [Index corruption or concurrent loss] → digest verification fails closed; orphan cleanup during eviction; worst case is an explicit unknown-handle error with remedy, never fabricated content.
- [Footer overhead on every compressed run] → one line, measured in acceptance evidence.
- [CLI/API drift] → no new hook events or Codex surfaces are used; recall is an ordinary native command invoked explicitly.

## Migration Plan

Additive only: new subcommand, new storage area, one footer line on the already-retained path. No configuration, component or trust migration. Rollback is the existing update/recover lifecycle reinstalling the prior adapter build; pack files are inert data and can be deleted manually without affecting any other path.