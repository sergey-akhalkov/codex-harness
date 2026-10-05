## Why

The catalogue records Delphi as a retired candidate even though the shared
Serena installation already contains pasls 0.2.0 with a matching FPC 3.2.2
compiler and source tree (the prerequisites pasls uses for CodeTools). A
Pascal/Delphi project therefore has no managed backend: discovery emits no
Pascal record, the generated Serena configuration pins no launch command, and
worker startup refuses the project before a language server can serve symbols
or references. The user approved restoring Pascal/Delphi to the managed
selection from the existing installation, without new downloads.

## What Changes

- Promote `delphi` from `retired_language_candidates` to a reuse-only row of
  the managed `languages` selection in `global/code-tools.json`.
- Discover the shared `PascalLanguageServer` cache layout (`pasls.exe`, the
  installer version record, and `prerequisites/<fpc>/bin/<target>/fpc.exe`
  with its sibling `source` tree) and adopt it only when every part is present
  and unambiguous; an absent executable, a missing or ambiguous FPC pairing,
  or an absent or unusable version record leaves the row missing, broken or
  incomplete, with observed fingerprints and no independent upstream
  per-binary integrity claim.
- Generate the Pascal launch setting with the pinned pasls executable plus
  `pp`/`fpcdir` entries so pasls CodeTools uses the matching FPC driver and
  sources, and pin the adopted pasls directory ahead of `PATH` for Serena
  workers so the backend resolves the shared binary instead of provisioning a
  copy into the owned home.
- Support the exact GitHub releases metadata endpoint in dependency planning
  so the reuse-only row keeps `dependencies plan` and `dependencies apply
  --check` working.
- Cover adoption, absent or ambiguous prerequisite and invalid version-record
  cases in native tests, exercise a managed MCP session on a synthetic
  CP1251/CRLF Pascal fixture, and keep docs/specs coherent with explicit
  non-claims for Delphi SDK support and clean compilation.
- Verify the row through the supported registry/install lifecycle: the
  `install/update --code-tools-only` path must write a
  `harness/code-tools.json` whose adopted Pascal record feeds the worker
  configuration generator, and the managed MCP session must consume that
  lifecycle-produced registry shape rather than only a hand-supplied one.
- Extend the deployment semantic acceptance (`mcp serena-check`) to exercise
  Pascal when the registry adopted the row, so the shared lifecycle verifies
  the restored selection on machines with the verified installation.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `global-code-tools`: reuse-only managed Pascal/Delphi selection with
  verified pasls + FPC prerequisites, no session provisioning, and stated
  FPC/CodeTools limits.

## Impact

Dependency discovery, Serena configuration and worker startup, dependency
metadata planning, catalogue, tests and documentation. No download, install
or update runs during a session; missing prerequisites only reduce the
selection. Delphi SDK compilation remains user-owned.
