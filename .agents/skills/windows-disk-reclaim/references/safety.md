# Safety contract

The helper deletes files only when every check below succeeds. Agents must not add a parallel deletion path.

## Allowlist categories

Resolved at runtime from environment and known Windows locations on the requested drive. Missing or off-drive paths are omitted, not guessed.

| id | Typical location | Age gate |
| --- | --- | --- |
| user-temp | process temp directory | 48 hours, or `--min-age-hours` if larger |
| user-temp-local | `%LOCALAPPDATA%\Temp` when distinct | same |
| windows-temp | `%WINDIR%\Temp` | same |
| thumbnails | Explorer thumbnail cache directory | none; skip in-use |
| wer-queue | Windows Error Reporting queue | 24 hours |
| internet-cache | `INetCache` | 48 hours |
| shader-cache | DirectX `D3DSCache` | 48 hours |
| crash-dumps-user | `%LOCALAPPDATA%\CrashDumps` | 7 days |
| minidumps | `%WINDIR%\Minidump` | 7 days |
| delivery-optimization | Delivery Optimization cache | 48 hours |
| recycle | Recycle Bin via `SHEmptyRecycleBin` | only with `--empty-recycle --apply` |

Thumbnail deletion is best-effort: Explorer often has the databases open.

## Per-file guards

1. Path is under a canonical allowlist root on the requested drive (case-insensitive, reparse-resolved).
2. Entry is an ordinary file, not a directory, junction, symlink, or other reparse point.
3. File name is not a denied system name (`pagefile.sys`, `hiberfil.sys`, `swapfile.sys`, `ntuser.dat`, boot files).
4. Last-write age meets the category threshold (recent temp files stay).
5. `remove_file` failures, including sharing violations, skip the file and continue.

Directories are never removed. Junctions are never followed. A junction inside temp that points at Documents fails the canonical-prefix check.

## Forbidden

- Ranking files by size and deleting the top N
- User content trees, including Downloads
- Arbitrary `--root` deletion
- `Windows.old`, WinSxS/component-store reset, hibernation, pagefile
- Developer caches (`cargo`, `npm`, `NuGet`) unless a later explicit category is added to the helper

If the allowlist cannot free enough space, report the snapshot and stop. Broader deletion is a user-owned file operation, not this skill.
