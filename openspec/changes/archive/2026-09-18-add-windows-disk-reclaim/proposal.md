# Add Windows C: snapshot and allowlist reclaim

## Why

Coding agents on Windows need a fast answer to what on `C:` consumes space and a way to free space without deleting user work. Recursive PowerShell walks are slow; deleting the largest files is unsafe.

## What Changes

- Add a Rust helper that snapshots drive usage by reading the NTFS `$MFT` when the volume can be opened, with a hard-link-aware walk fallback.
- Reclaim space only from a built-in allowlist of regenerable caches, with age, reparse, canonical-prefix and deny-name guards. Recycle Bin emptying uses the Shell API and is opt-in.
- Deliver a discoverable skill that runs that helper and forbids size-ranked deletion.

## Capabilities

### New Capabilities

- `windows-disk-reclaim`: fast system-drive usage snapshot and allowlist-only reclaim for Windows.

### Modified Capabilities

- None.

## Impact

New workspace crate `windows-disk-reclaim`, skill `.agents/skills/windows-disk-reclaim`, and crate tests. Kit install already links every skill directory. No new third-party service, MCP, or extra runtime beyond the existing Rust toolchain. Snapshot and reclaim output contain local paths and stay off Git.
