# Design

## Context

The fastest complete NTFS inventory is a sequential read of `$MFT`, not a directory walk. WizTree-style tools open the volume, parse file records, and aggregate unique allocated sizes. That needs volume access. Everyday sessions may lack it, so a walk that skips reparse points and deduplicates file indexes remains required.

Cleanup cannot use "largest first". User videos, VHDs, and databases dominate top-file lists. Microsoft Disk Cleanup already defines regenerable categories; this helper implements a conservative subset with explicit apply.

## Goals / Non-Goals

**Goals:**
- Snapshot `C:` in seconds when `$MFT` is readable, and still produce a useful heatmap when it is not.
- Report top directories and files plus allowlisted reclaimable bytes.
- Delete only allowlisted files after `--apply`, never by size rank.
- Keep the skill globally discoverable through the existing skills inventory.

**Non-Goals:**
- Cleaning developer package caches, `Windows.old`, WinSxS `/ResetBase`, hibernation, or the pagefile.
- A resident background indexer or USN catch-up daemon.
- Non-Windows hosts.
- Installing a prebuilt helper during `codex-harness install` (first-use `cargo run --release --locked` is accepted).

## Decisions

- **MFT primary, walk fallback.** `auto` tries `FSCTL_GET_NTFS_VOLUME_DATA` and `$MFT` data runs, then falls back. Hard-link size is counted once by attaching each record to its first Win32 name.
- **Allowlist-only mutation.** CLI has no `--root` delete flag. Categories resolve from `TEMP`, `LOCALAPPDATA`, `WINDIR` and known Delivery Optimization paths on the requested drive.
- **Canonical prefix + reparse skip.** Junctions inside temp that point at user content fail `is_under` after `canonicalize`.
- **Age gates.** Default 48 hours for temps, longer for dumps, none for thumbnail databases (still skip in-use).
- **Recycle Bin is opt-in apply.** `SHEmptyRecycleBin` with no UI, only when `--empty-recycle --apply`.
- **JSON default.** Agents consume JSON; text is optional. Local paths are not checked in.

## Risks / Trade-offs

- MFT parsing ignores ATTRIBUTE_LIST extension records; a few files may miss ADS or split attributes. Walk fallback and volume free-space APIs still give a usable picture.
- Allowlist reclaim may free less than a human using Disk Cleanup with every box checked. That is accepted for the safety bar.
- First helper run compiles with the workspace lockfile. Disclose that cost; reuse `target/release` afterwards.

## Migration Plan

Add the crate and skill. Existing installs pick up the skill on the next core install/update because inventory links every directory under `.agents/skills`. No protocol or profile change.

## Open Questions

- None that block delivery. Developer-cache categories stay out until a later explicit request.
