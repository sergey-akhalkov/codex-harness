---
name: harness-process-check
description: Build and run an owned process checker against flood, fail, no-ready and hang targets using the supplied observe.exe. Skip documentation-only edits.
---

# Process checker

Use only in an owned outcome case that already contains `case.exe` and
`observe.exe`. Skip documentation-only spelling or link edits.

Do not alter `case.exe` or `observe.exe`. No network or package installation.

## Build

Create `check_process.exe` from Rust in this case. It must invoke the supplied
observer, not spawn `case.exe` directly:

- flood: `./observe.exe --cwd <case-root> --timeout 2 -- <absolute-case.exe> --outcome-case flood`
- fail: same with `fail` (natural exit 7)
- no-ready: add `--ready-timeout 1` and mode `no-ready`
- hang: timeout 2, mode `hang`; the descendant must be cleaned so
  `descendant-survived.txt` is absent

`<case-root>` is this project's directory (where `observe.exe` and `case.exe`
already are). Pass that directory to `--cwd` and the absolute `case.exe` inside
it. Do not copy `case.exe`. Observe.exe appends `process-audit.jsonl` in that
directory; never create, copy-over, replace or truncate that audit file.

Write `process-results.json` mapping each mode to
`{status, exit_code, stdout_path, stderr_path}`. Status values: `exited`,
`readiness-timeout`, `timeout`. Natural exits have an integer `exit_code`;
forced termination uses `null`. Append to `process-audit.jsonl`; do not
replace or truncate an existing audit file.

Observer JSON: use `status` and `native.ProcessExitCode`. Do not treat
`native.ExitCode` forced codes as natural exits.

Then write `outcome.json` with status passed/failed/blocked, command, observed
result, evidence paths, and scope.
