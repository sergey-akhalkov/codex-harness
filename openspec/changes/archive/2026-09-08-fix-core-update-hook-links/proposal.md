## Why

Open Codex sessions repeatedly report `Hook failed / hook exited with code 1` because the globally configured hook launcher has disappeared. A core-only install/update rebuilds its link inventory without previously connected code-tool hooks and removes those links as obsolete.

## What Changes

- Preserve connected hook definitions and their launcher during core install/update, including relocation and missing-link repair.
- Retain conflict protection, transactional rollback and the absence of hooks on a fresh core-only installation.
- Pass the same effective registry to command-hook and native MCP diagnostics so their shared runtime identity agrees.
- Restore the affected global installation and verify native command hooks outside this checkout.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `linked-global-kit`: a core update must preserve already connected hook capabilities.

## Impact

`tools/kit.psm1`, `tools/hook.ps1`, installer regression checks, installation documentation and the global link inventory. No dependency updates or model routing changes are required. An attempted diagnostic broker restart during investigation is recorded separately from the registry identity correction.
