# Native source diagnostics

2026-09-10, Windows x64, Rust/Cargo 1.97.1, original Codex CLI 0.153.4.
This completes the diagnostic port and alias acceptance in
[migration task 4.5](../../openspec/changes/migrate-harness-to-rust/tasks.md),
the native equivalent of [source diagnostics](../source-diagnostics.md).
The full migration and global cutover remain separate tasks.

## Entry points and effects

`codex-harness.exe diagnose` (also `check --diagnose`) emits the existing schema-1 source report plus
`model_calls: 0`. Options select the source, Codex/user/dependency homes, project,
upstream, profile and a 1–60 second RPC budget (30 by default). CLI paths resolve
against the caller's directory before creating the isolated consumer. With no
upstream/source override, the command reads the installation's recorded values.

The native core installer registers `codex-harness-check.exe` as a file link to
its immutable `codex-harness.exe`. That entry name dispatches directly to
Diagnose, and Check verifies its recorded manager target. It shares the existing
installation journal, owner checks and disconnect lifecycle; it is an alias,
not a sixth build binary. An owned old diagnostic `.ps1` registration is retired
by update; its source is preserved. Adopted objects retain their existing rules.
The machine's global installation still uses the script lifecycle until cutover.

[Installation observations](../../crates/harness-core/src/source_observation.rs)
read native/legacy ownership and merge the current data inventory. Missing or
invalid metadata leaves independent link observations available. These reads
perform no repair, registration, package acquisition or CLI execution.
[The manager](../../crates/codex-harness/src/source_diagnostics.rs) then uses
[the shared read transport](../../crates/codex-harness/src/native_read_rpc.rs)
for fixed initialize/config/requirements/skills calls and a separate native
profile parse. Each process enters a 512 MiB / 50% CPU Windows job before resume.
RPCs share one deadline, enforce record/output limits and verify the complete
reply stream after owned shutdown. Consumers start in their private temporary
directory, with the requested project passed explicitly to read methods.

[The view](../../crates/codex-harness/src/source_diagnostics_view.rs) retains the
eight permitted preferences, source paths and skill name/path/scope/enabled.
It withholds developer text, unknown preference values, skill descriptions and
raw parser errors. Missing required observations cannot supply effective winners.
Profile discovery/trust/skill changes and managed requirements suppress all
reconstructed winners. Windows trust ancestry normalizes dot segments, handles
drive roots and uses ordinal case-insensitive comparison including Unicode.

The report still says `inferred-from-native-layers`, not a full selected-session
configuration dump. Skills belong to the base consumer; existing session/MCP
freshness remains unknown. Only CLI 0.153.4's profile reconstruction contract is
accepted. Temporary profile links and private raw RPC files are removed after
the owned jobs stop. No thread/turn request, model request or project command is
part of this protocol. Status remains a JSON field: incomplete observations do
not become a process exit failure.

## Acceptance evidence

Detailed local outputs use the `source-diagnostics-*` / `source-observation-*`
names in the core lifecycle evidence directory linked from
[core installation](rust-core-installation.md).

- `source-diagnostics-real-first.stdout`: actual original app-server reads in
  an owned legacy source/home/Git project outside the checkout, one passing test,
  56.95 seconds. `%TEMP%/source-observation проверка-F4ThQJ` retains sanitized
  reports for clean/conflicting/restored/untrusted/profile-context/invalid-TOML
  cases. It checks simultaneous project overrides, duplicate skills, retargeted
  links, explicit false, disabled layers, privacy and config/auth/history/metadata
  preservation. A configured native fixture SessionStart hook did not run.
- `source-diagnostics-alias-first.stdout`: actual native install/update/Check,
  invocation through the installed diagnostic alias with inferred source/upstream,
  and disconnect outside the checkout; one passing test, 107.53 seconds,
  `%TEMP%/native-core проба-PV0hNp`. Diagnose preserved metadata; disconnect removed
  the alias and preserved source/upstream. This uses an owned miniature build
  receipt and real binaries, not a complete globally activated candidate.
- `source-diagnostics-legacy-alias.stdout`: legacy registration upgrade with
  the original CLI and actual manager/launcher binaries passed in 92.93 seconds,
  `%TEMP%/native-core проба-gkRnkw`. Preview preserved the old diagnostic link and
  metadata; update retired its owned `.ps1` link, preserved its source and the
  adopted profile, and registered the new `.exe` alias. Inherited hook repair and
  rejection of a foreign replacement also passed in this owned lifecycle test.
- `source-diagnostics-final-consumers.stdout`: 30 binary unit tests, three
  installation-state cases, eight launcher cases, ten outcome-discovery cases
  and four source-diagnostic CLI cases passed. The two explicit native tests
  were ignored in this run; separate actual source-consumer evidence is above.
  Cases include the relative project directory, `check --diagnose` compatibility
  and refusal of duplicate/mixed diagnostic/core selectors before execution.
  Failures include
  timeout, malformed/error/wrong-ID/truncated replies, private-output suppression,
  unchanged user files and verified child exit. Invalid options do not start the
  upstream fixture.
- `source-diagnostics-view-fixed.stdout`: all six interpretation tests passed
  after required requirements-field validation and Windows trust ancestry fixes.
- `source-observation-second.stdout`: owned legacy source/link observations,
  absence preservation and independent evidence on malformed metadata passed.
- `source-diagnostics-final-core.stdout`: 189 library tests passed, including
  read-only link observation and the native alias's registration/Check rules;
  26 explicit fixtures were ignored.
- `source-diagnostics-final-registration.stdout`: 27 registration/finish cases
  passed, with seven explicit fixture cases ignored. Final Rust formatting and
  both packages' all-target Clippy checks passed with warnings denied, recorded
  in `source-diagnostics-final-format.log` and `source-diagnostics-final-lint.stderr`.
  `source-diagnostics-acceptance.json` retains the exact commands, selected source
  and manager hashes, reuse boundaries and evidence limits for this checkpoint.

Earlier failures are retained: invalid quoted Rust literals from the initial
worker file, a test fixture using `wrong-id` instead of `unknown-id`, a legacy
test record using verbatim paths instead of its producer's ordinary drive paths,
and a missing test helper import. Clippy also rejected a needless forward scan;
the reverse winner lookup passed the final view/consumer checks. These failed
attempts are not passing evidence.

An independent Grok source review found no material P0/P1 issue in the changed
invocation, private temporary state, shared RPC and link-observation boundaries.
It reused the prior primitive review and did not rerun checks or review the view
again. Serena was unavailable in that child; its review used direct source.
The protocol's zero-model claim is not a network monitor. Startup with configured
MCP servers and full global delivery remain separate consumer evidence.

The source migration's complete format/lint/integration checks and global
delivery state remain governed by the migration
task list; this report does not close those steps by assertion.
