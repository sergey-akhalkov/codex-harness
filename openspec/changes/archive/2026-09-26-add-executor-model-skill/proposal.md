# Add executor-model skill and DeepSeek executor binding

## Why

User decision (2026-09-25): executors must run the `ds` profile (DeepSeek
V4.1-Flash at `max`) instead of `xai`, and future executor-model switches must
be one reusable action owned by this repository, not machine-local authoring
that is lost when the workstation changes.

## What Changes

- `global/orchestration.toml`: `executor_profiles = ["xai"]` becomes `["ds"]`.
  The installed launcher reads the kit checkout config live; no reinstall is
  required.
- Consumer switch verified outside this change's write scope: one consuming
  checkout now also sets `["ds"]` through the same one-line edit.
- New kit-owned skill `.agents/skills/executor-model/` (source-linked into user
  skills like other kit skills): a canonical one-action PowerShell block that
  validates the target profile is installed, prints its model/provider/effort,
  rewrites only the `executor_profiles` line in the named checkouts and reads
  every change back with file and line. It ships no executable file, so the
  Rust executable-ownership boundary stays intact.
- `AGENTS.md`: reusable skills, helpers and configuration are authored only in
  this repository; machine-local skill and config directories may hold only
  lifecycle-produced links, builds and state.
- The harness-core orchestration tests and current-routing statements are
  synchronized to the committed `ds` executor profile.

## Impact

Executors spawned after the switch run DeepSeek V4.1-Flash at `max` through the
installed `ds` profile. Lead and successor-lead profiles are unchanged.
