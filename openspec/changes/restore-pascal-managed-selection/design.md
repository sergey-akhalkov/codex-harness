## Context

See [proposal.md](proposal.md). Serena 1.7.0's `PascalLanguageServer`
(pasls 0.2.0) resolves its binary in this order: `pasls` on `PATH`, then
`<SERENA_HOME>/language_servers/static/PascalLanguageServer`, then
provisioning. Harness workers set `SERENA_HOME` to an owned per-worker home,
so without a pin the backend would download pasls into that home. The
explicit-launch setting the harness uses for other backends
(`ls_base_cmd`/`ls_args`) is ignored by this legacy backend implementation,
while its `ls_specific_settings` reader does consume `pp`/`fpcdir` and passes
them to pasls as `PP`/`FPCDIR`.

## Decision

- Discovery adopts only the observed shared layout: `pasls.exe`, the
  installer's `.meta/version` record parsed as a stable dotted version, and a
  unique `prerequisites/<fpc>/bin/<target>/fpc.exe` with its sibling `source`
  directory. The record fingerprints both binaries, keeps `update_safe`
  false, and states that the version record is installer-written identity,
  not a published per-binary hash.
- The generated `pascal` (with the `delphi` settings alias) launch setting
  pins `ls_base_cmd` to the verified pasls executable and adds `pp`/`fpcdir`;
  the worker environment prepends the pasls directory to `PATH`. Under
  Serena 1.7.0 the `PATH` pin is what keeps startup download-free, and the
  pin stays correct if a later Serena honors `ls_base_cmd` for this backend.
- The catalogue row is `required: false` with a conditional note: absence is
  an accepted state (`conditional-absent` in planning) and nothing is
  provisioned. A missing or tampered installation refuses affected projects
  before a worker starts instead of falling back to provisioning.
- Metadata planning supports the exact GitHub releases endpoint so
  `dependencies plan` and `apply --check` keep working; a newer upstream tag
  is reported, never applied by the shared installer.

## Risks

- A future Serena may rename the cache directory or change the pasls settings
  contract: discovery and the launch settings fail closed (missing or
  unverified) instead of provisioning, and the change is re-evaluated then.
- Legacy CP1251 sources: the managed session is exercised with a project
  `encoding: cp1251` fixture; FPC/CodeTools navigation is partial and is not
  reported as Delphi compiler support or clean compilation.

## Verification

Focused native checks plus one managed MCP session on synthetic owned Pascal
units (`get_symbols_overview`, `find_symbol`, `find_referencing_symbols`) with
the accepted shared registry. The shared pasls binary and the user Serena
config are compared before and after, and the owned home is asserted to
contain no provisioned pasls copy.

The registry side is checked through the supported lifecycle rather than only
a hand-supplied file: a focused lifecycle test runs `Mode::Install` against a
synthetic dependency home and asserts that the written
`harness/code-tools.json` carries the adopted Pascal row, and that
`serena_configuration::prepare` turns that written registry into a worker home
with the pinned `pascal` launch setting (`pasls` plus `pp`/`fpcdir`). The
session exercise then uses the same registry shape the lifecycle writes, and
the deployment semantic acceptance (`mcp serena-check`) exercises Pascal on
machines whose registry adopted the row, so the shared lifecycle verifies the
restored selection rather than only the fixture tests. The global
install/update on the shared machine stays with the lead.
