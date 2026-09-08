## Why

Automatic diagnostic hooks repeatedly exhaust their budgets during ordinary work. Unfinished files re-expand already checked cohorts, and analysis consumes the time needed to validate results before delivery; the independent pre-edit journal also has waits outside its shared deadline.

## What Changes

- Make successive bounded checks complete remaining work for unchanged inputs without rechecking already verified files.
- Reserve time for final source verification and retain explicit unresolved status for actual timeouts and concurrent edits.
- Bound pre-edit traversal and database waits within the existing native hook limit.
- Deliver through the existing globally linked kit and verify real hooks outside this checkout.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `automatic-lsp-diagnostics`: require progress across bounded retries and an end-to-end pre-edit deadline.

## Impact

Diagnostic service, SQLite journal, targeted regression tests and diagnostic operating documentation. Existing model routing, credentials and unrelated installed consumers remain outside this change.
