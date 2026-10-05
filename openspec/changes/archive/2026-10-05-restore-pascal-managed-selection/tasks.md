## 1. Restore the managed selection

- [x] 1.1 Promote `delphi` to the managed `languages` selection as a reuse-only
  conditional row and map the exact GitHub releases metadata endpoint so
  read-only planning keeps working.
- [x] 1.2 Discover the shared pasls layout (`pasls.exe`, `.meta/version`,
  matching FPC `bin/<target>/fpc.exe` plus `source`) with positive, absent or
  ambiguous, and invalid-version cases; keep non-adopted rows out of the
  generated settings.
- [x] 1.3 Generate the pinned pascal launch setting (`ls_base_cmd`, `pp`,
  `fpcdir`) and pin the adopted pasls directory into the worker `PATH`; keep
  session startup download-free.
- [x] 1.4 Cover discovery, worker configuration/pin, planning and the
  lifecycle-written registry in focused native tests, and extend the
  deployment semantic acceptance with a conditional Pascal case.
- [x] 1.5 Run the focused native suites, the clippy gate and the source and
  ownership hygiene checks.
- [x] 1.6 Exercise a managed MCP initialize plus Pascal symbol overview, find
  and references on a synthetic CP1251/CRLF fixture with the registry produced
  by the supported discovery/lifecycle inputs, including the deployment
  semantic acceptance on that registry.
- [x] 1.7 Lead: deploy through the installation lifecycle and verify a
  separate private Delphi consumer; archive the change.

## 2. Documentation

- [x] 2.1 Update the language-selection matrix and reuse-only explanation in
  `docs/code-tools.md`.
- [x] 2.2 Sync the delta requirements into
  `openspec/specs/global-code-tools/spec.md` and validate the change.
- [x] 2.3 Correct the adoption-failure and identity wording to the exact
  checked cases: missing or ambiguous prerequisites and absent or unusable
  version records, observed fingerprints without an independent upstream
  integrity guarantee, and existence-only startup checks.
