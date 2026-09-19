# windows-disk-reclaim Specification

## Purpose

Give Windows coding agents a fast inventory of what occupies the system drive and a mutation path that can only delete regenerable allowlisted files.

## ADDED Requirements

### Requirement: Fast NTFS snapshot

The helper SHALL snapshot the requested Windows drive by reading NTFS `$MFT` when the volume can be opened, and SHALL fall back to a directory walk that skips reparse points and counts hard-linked files once. The snapshot SHALL report elapsed time, volume totals, top directories, top files, and allowlisted reclaimable bytes. Snapshot SHALL NOT delete files.

#### Scenario: Volume MFT is readable

- **WHEN** `snapshot --drive C --method auto` runs with volume access
- **THEN** the report `method` is `mft` and includes top directories and files with allocated sizes

#### Scenario: Volume MFT cannot be opened

- **WHEN** `snapshot --drive C --method auto` cannot open the NTFS volume
- **THEN** the helper walks the drive without following reparse points and still returns a snapshot rather than deleting anything

### Requirement: Allowlist-only reclaim

Reclaim SHALL delete files only under resolved allowlist roots on the requested drive after `--apply`. Dry-run SHALL report candidates without deleting. Reclaim SHALL skip reparse points, denied system names, recent files under the age gate, and files that cannot be removed. The CLI SHALL NOT accept a custom deletion root. Size ranking SHALL NOT select deletion targets.

#### Scenario: Dry-run leaves files in place

- **WHEN** reclaim runs without `--apply` against an allowlisted fixture that contains an old temp file and a sibling documents file
- **THEN** the old temp file is reported as a candidate and both files still exist

#### Scenario: Apply deletes only allowlisted old files

- **WHEN** reclaim runs with `--apply` against that fixture
- **THEN** the old allowlisted file is removed, a recent allowlisted file remains, and the documents file remains

#### Scenario: Off-drive category is refused

- **WHEN** reclaim is asked to use a category path that is not on the requested drive
- **THEN** it fails without deleting files

### Requirement: Skill forbids unsafe cleanup

The pack SHALL ship a discoverable skill that runs the helper for C: usage and cleanup, instructs the agent not to delete snapshot-ranked files or user content, and requires `--apply` plus user intent before mutation.

#### Scenario: Skill is in the managed catalogue

- **WHEN** the kit inventory lists `.agents/skills`
- **THEN** `windows-disk-reclaim` is a skill directory with `SKILL.md` whose description routes C: space and cleanup requests to this workflow
