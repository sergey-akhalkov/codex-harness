---
name: windows-disk-reclaim
description: Snapshot the largest files and directories on Windows drive C via NTFS $MFT and reclaim space using only allowlisted regenerable caches. Use when C: is low on free space or a user asks to clean the system drive. Skip project source cleanup, user documents, and any size-ranked deletion.
---

# Windows C: snapshot and safe reclaim

Free space on `C:` by measuring first, then deleting only files inside the helper's built-in allowlist. Never delete the largest files from a snapshot. Size ranking is diagnostic, not a deletion key.

## Helper

The executable is workspace crate `windows-disk-reclaim`. Follow this skill directory's install link to the kit source, then run the crate:

```powershell
$skillPath = Join-Path $env:USERPROFILE '.agents\skills\windows-disk-reclaim'
if (-not (Test-Path -LiteralPath $skillPath)) { throw 'windows-disk-reclaim skill is not linked' }
$skill = Get-Item -LiteralPath $skillPath
$source = if ($skill.Target) { [string]$skill.Target } else { $skill.FullName }
$root = (Resolve-Path (Join-Path $source '..\..\..')).Path
$exe = Join-Path $root 'target\debug\windows-disk-reclaim.exe'
$release = Join-Path $root 'target\release\windows-disk-reclaim.exe'
if (Test-Path -LiteralPath $release) { $exe = $release }
if (-not (Test-Path -LiteralPath $exe)) {
  cargo build --locked -p windows-disk-reclaim --manifest-path (Join-Path $root 'Cargo.toml')
  $exe = Join-Path $root 'target\debug\windows-disk-reclaim.exe'
}
```

Prefer a built binary. First use may compile. Keep `--locked`. Do not add deletion flags beyond the helper CLI.

## Workflow

1. Run a snapshot. Prefer MFT; the helper falls back to a junction-skipping walk when the volume cannot be opened:

```powershell
& $exe snapshot --drive C --top 25 --format json
```

2. Report volume used/free, elapsed time, `top_level`, top directories/files, and snapshot `reclaimable_*` (MFT upper bound). Do not treat top files as cleanup targets.
3. If the user asked only what is large, stop after the snapshot.
4. If the user asked to free space, run a dry-run unless they already confirmed apply:

```powershell
& $exe reclaim --drive C --format json
```

5. Apply only the allowlist after an explicit request to clean. Recycle Bin emptying is separate and still uses the official API:

```powershell
& $exe reclaim --drive C --apply --empty-recycle --format json
```

6. Snapshot again and report the free-space delta. In-use files are skipped; that is success, not a reason to broaden deletion.

Read [safety](references/safety.md) before any apply. Read [commands](references/commands.md) for flags and output fields.

## Hard limits

- Do not `Remove-Item`, `rm`, `del`, Explorer send-to-recycle, or DISM/cleanmgr except by telling the user those tools exist outside this skill.
- Do not delete from snapshot ranking, Downloads, Documents, Desktop, user profiles, `Program Files`, or `C:\Windows` except through the helper allowlist.
- Do not pass a custom root to deletion. The CLI has no `--root` delete option.
- Do not disable hibernation, shrink the pagefile, remove `Windows.old`, or run `DISM /ResetBase` as part of this skill.
- `--apply` is the only mutation. Without it the helper only reports.
