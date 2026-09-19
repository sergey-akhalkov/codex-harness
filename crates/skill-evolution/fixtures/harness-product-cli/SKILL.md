---
name: harness-product-cli
description: Verify a product CLI native entrypoint and version records in an owned outcome case. Skip documentation-only spelling, link or factual edits.
---

# Product CLI verification

Use only for native product CLI and version-record work in the current owned case.
Skip this skill for documentation-only spelling or link edits. Do not start test
suites, install packages, or delegate.

## Steps

1. Read the case notes and any existing validation record.
2. Build generated output with the documented native target:
   `./case.exe --outcome-case build`.
3. Exercise the documented entrypoint: `./case.exe --outcome-case cli`.
   It must print the current source version from `source.json`.
4. Ensure `built.json` matches `source.json` after the build.
5. If a historical validation record exists, keep the earlier version and stdout
   numbers, mark that record as historical or previous, and add the current
   version as a distinct line. Do not replace history with only the new number.
6. Write `outcome.json` with status passed/failed/blocked, command, observed
   result, evidence paths, and scope. Never count a skipped check as passed.

Do not alter `case.exe`. No network or package installation.
