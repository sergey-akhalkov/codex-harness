# Design

## Context

See proposal.md. The adopted pasls v0.2.0 initializes its own workspace symbol
index before answering `initialize`. Its `TInitialize.Process` collects workspace
directories independently of Serena ignore rules and scans them when
`workspaceSymbolProvider` is enabled. The observed failure is an LSP `-32603`
response after loading thousands of generated source copies, not evidence that
the broker killed a worker for exceeding its job limit.

## Decisions

Set `initializationOptions.workspaceSymbols` to false in the managed Pascal
settings. Serena 1.7.0 supports this standard custom initialization-options seam;
its managed symbol tools enumerate eligible files and use document symbols rather
than `workspace/symbol`. Keep document symbols and CodeTools navigation enabled.
Serialize the structured option in the existing generated YAML owner.

Increasing memory only postpones the failure. Hard-coding a build directory list
would duplicate consumer exclusions and can hide legitimate source. Reimplementing
Git/YAML ignore matching or patching the foreign package is unnecessary for the
redundant index; use its existing configuration instead.

Upstream contracts inspected at
[pasls v0.2.0](https://github.com/zen010101/pascal-language-server/blob/v0.2.0/src/serverprotocol/PasLS.General.pas)
and [Serena v1.7.0 initialization options](https://github.com/oraios/serena/blob/v1.7.0/src/solidlsp/initialize_params.py),
and compared with installed source. Private reproduction inputs/logs stay local.

## Risks / Trade-offs

Pasls still collects search directories and can load units requested by CodeTools;
this does not implement universal backend ignore filtering. Explicit project
initialization options retain their upstream override semantics. Verify actual
symbol search and cross-unit references with the redundant index disabled.
Empty LSP diagnostics do not certify a Delphi build.

## Migration

Deploy through the native lifecycle. Fresh workers receive the setting; existing
workers need their owning connection restarted or their normal retirement.
Preserve rollback through the installation's existing build history.
