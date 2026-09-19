# Tasks

## 1. Helper

- [x] 1.1 Add workspace crate `windows-disk-reclaim` with snapshot, MFT reader, walk fallback and allowlist reclaim CLI.
- [x] 1.2 Cover allowlist dry-run/apply, off-drive refusal, MFT record parsing and fixture walk ranking with `cargo test -p windows-disk-reclaim`.

## 2. Skill

- [x] 2.1 Add `.agents/skills/windows-disk-reclaim` with safety and command references that forbid size-ranked deletion.
- [x] 2.2 Validate the skill frontmatter and confirm inventory will discover the new directory.

## 3. Live check

- [x] 3.1 Run `snapshot --drive C --format json` from this machine and record method plus elapsed time without storing local paths in Git.
- [x] 3.2 Run `reclaim --drive C` dry-run, then apply only if the candidate set is allowlisted; report free-space delta without checking in output.
