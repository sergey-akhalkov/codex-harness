# Helper commands

Default `--drive C`. Default `--format json`. Without `--apply` nothing is deleted.

```text
windows-disk-reclaim snapshot [--drive C] [--top 25] [--method auto|mft|walk] [--format json|text]
windows-disk-reclaim reclaim  [--drive C] [--apply] [--empty-recycle] [--min-age-hours 48] [--format json|text]
windows-disk-reclaim categories [--drive C] [--format json|text]
```

## Snapshot

`auto` opens the NTFS volume and reads `$MFT` sequentially (WizTree-style). That path needs volume access, usually an elevated process. On failure `auto` walks the drive and skips reparse points. `mft` does not fall back. `walk` skips the MFT.

JSON fields: `drive`, `method`, `elapsed_ms`, `volume.{total,free,used}_bytes`, `file_records`, `top_level`, `top_directories`, `top_files`, `reclaimable_bytes`, `reclaimable[]`.

`top_level` is unique allocated size of each child of the drive root. Snapshot `reclaimable_*` is an MFT subtree estimate for allowlist roots without the age gate. Run `reclaim` without `--apply` for the age-gated candidate set.

Treat `elapsed_ms` as the speed evidence. Do not commit snapshot JSON; it contains local paths.

## Reclaim

Dry-run is the default. `--apply` deletes allowlisted files that pass [safety](safety.md). `--empty-recycle` is ignored unless `--apply` is also present.

JSON fields: `apply`, `candidate_*`, `deleted_*`, `freed_bytes`, `skipped_files`, `recycle_emptied`, `categories[]`.

`skipped_files` includes in-use, access-denied, reparse, and deny-listed names. Do not retry those with a broader tool.

## Categories

Prints the allowlist roots resolved on this machine. Use it to explain what apply would touch. Paths are local; do not copy them into the pack.
