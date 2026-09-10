# Native dependency observations

2026-09-10, Windows x64, Rust/Cargo 1.97.1. This records the implementation and
owned/outside-checkout CLI acceptance of [migration task 5.1](../../openspec/changes/migrate-harness-to-rust/tasks.md).
Native discovery and read-only release planning now have actual CLI consumers.
The installed script lifecycle and complete dependency/update integration remain
unfinished; release comparison does not prove a staged package's compatibility.

`codex-harness.exe dependencies discover --source CHECKOUT` reads the current
`global/code-tools.json` selection: Serena, Codebase Memory, Graphify, Nuphus,
Python and Rust. `--user-home` selects dependency roots, and an alternate home
does not inherit ambient PATH or tool directories by default. Explicit root
options and `--include-process-environment` / `--no-process-environment` control
that boundary. Default discovery starts no package, model or project process.

[The reader](../../crates/harness-core/src/dependency_discovery.rs) checks npm
package names and entrypoint containment, matching native platform versions,
UV metadata and console entrypoints, local wheel RECORD hashes and Rust toolchain
metadata. Missing, broken, modified, ambiguous and incomplete observations stay
distinct. Aliases of one installation are deduplicated. Local Nuphus variants are
preserved and reported as modified. Installed presence does not establish a
working MCP operation or project language support.

Native Codebase Memory/Nuphus payloads are selected without their potentially
installing JS wrappers. UV observations name the package-owned console executable
instead of generating Python `-c` code. The future adapters still need their own
protocol, provisioning-suppression and global-consumer acceptance. npm fingerprints
and wheel RECORD are local installation evidence, not trusted release signatures.

JSON reads are capped at 4 MiB. Package hashes stream through a bounded buffer;
manifest fields and their fingerprint come from the same bytes, preserving BOM
and line endings in the hash. Only selected metadata is returned; package bodies,
Graphify credentials/headers and parser errors are withheld. Selected package
metadata and entrypoint paths must remain within that package. Default consumer
state remains `not-checked`, so this report alone cannot authorize an update.

`--full-records` expands the wheel selection beyond Python sources and package
metadata. `--probe-versions` separately requests the resolved native Rust analyzer's
version. [That probe](../../crates/harness-core/src/dependency_probe.rs) uses the
existing 512 MiB / 50% CPU management job, an eight-second deadline, bounded private
output and a temporary working directory with private Rust/Cargo homes. It sets
`RUSTUP_AUTO_INSTALL=0`, the [documented Rustup opt-out](https://rust-lang.github.io/rustup/environment-variables.html),
and accepts only a bounded Rust analyzer version line. Failures preserve the
installation observation without publishing raw output. When probes are requested,
the report does not claim an exact process count.

`--processes` adds [native host observations](../../crates/harness-core/src/dependency_process.rs)
even for an alternate dependency home, without starting a shell or reading process
environments. It matches installation paths and exact native companion payloads;
shared interpreter paths alone do not attribute a consumer. Only verified PID,
creation time, executable and match evidence leave the reader. Arguments stay in
private memory. Unsupported architectures, denied reads, unstable identities and
unavailable required scope produce `incomplete` or `unavailable`, retaining positive
observations. Paths are matched lexically with Windows case/boundary rules; unknown
filesystem aliases are not assumed equivalent. This is advisory evidence, not
authority to update a shared installation or terminate a process.

The command-line reader uses the [version-sensitive Windows PEB contract](https://learn.microsoft.com/en-us/windows/win32/api/winternl/ns-winternl-peb),
checks its layout against the current process and supports native x64 on the
observed Windows 10.0.26200 host. Its PID scan is capped at 65,536 entries and
20 seconds; argument buffers are bounded and are not persisted.

## Current evidence

Local logs are in the core lifecycle evidence directory linked from
[core installation](rust-core-installation.md), with the `dependency-discovery-`
prefix. These runs use the debug manager built from current dirty source, not a
complete globally activated immutable candidate.

- `first-cli.stdout`: all six actual CLI integration tests passed in 13.68 seconds
  after a 19.70-second compile. Owned fixtures cover alternate-home isolation,
  package identity, missing/mismatched native payloads, modified variants, alias
  deduplication, ambiguity, escaping entrypoints, malformed partial input, BOM/hash
  consistency, Graphify credential withholding, UV RECORD/console identity and
  opt-in version success/error/timeout with private-state cleanup.
- `actual-default.json`: outside-checkout read of the installed dependencies,
  no package execution. Serena 1.7.0 matched 152 selected RECORD files; Graphify
  0.9.55 matched 88. Codebase Memory 0.10.8 and Python backend 1.39.10 were found;
  Nuphus 0.2.2 was correctly retained as a modified native variant. Rust analyzer
  was found with version left unknown until explicitly queried.
- `actual-full.json`: 222 Serena and 232 Graphify RECORD entries matched, with
  no reported issue; this still does not establish upstream authenticity.
- `actual-probes.json`: the bounded explicit version read returned
  `rust-analyzer 1.97.1 (8bab26f4 2026-07-14)` with exit zero.
- `dependency-wheel-second.stdout`: all 14 wheel tests passed after strict CSV
  state handling, preserving empty quoted EOF fields and rejecting mid-field
  quotes. Cases include defaults/full selection, unsupported hashes, malformed
  and oversized inputs, missing/modified files, contained parent paths and
  file/directory link escapes. The first test compile's ambiguous expected-value
  type and the two Clippy findings remain recorded as failed attempts.
- `dependency-process-first-unit.stdout`: nine integrated process-reader cases
  passed, with two explicit fixtures ignored. The separately invoked owned-child
  privacy case passed in `dependency-process-owned-child.stdout`.
- `dependency-process-cli.stdout`: all seven actual CLI cases passed after process
  inspection was wired. The owned consumer remains alive after observation; its
  private arguments/environment do not enter the report. The parent reproduced
  the senior worker's private-source process checks against the actual Cargo build.
- `dependency-process-actual.json`: the installed dependency observation outside
  the checkout found 22 path-matching Serena consumers in that snapshot. All six
  consumer scopes were `incomplete`; zero matches for the other five records do
  not prove absence and cannot authorize their replacement.

The initial core compile stopped on the not-yet-created worker module. The final
process integration passed formatting and both packages' all-target Clippy.
`dependency-process-combined-core.stdout` records 212 passing core tests, zero
failures and 28 explicit cases ignored in 69.77 seconds. The CLI and separately
invoked owned-child checks above supplement that default suite. That discovery
checkpoint did not yet include release planning; the integrated acceptance below
closes task 5.1's port. The dependency lifecycle and global activation remain open.

The local `dependency-discovery-acceptance.json` records the relevant discovery
module/catalogue hashes and the tested debug manager. It explicitly distinguishes
that binary from later, unbuilt planning edits in the CLI.

## Release planning implementation

`codex-harness dependencies plan` accepts the discovery root/probe options and
explicitly requests public release metadata. Its [planner](../../crates/harness-core/src/dependency_plan.rs)
preserves modified, ambiguous, broken and incomplete installations, rejects
cross-package version comparisons, and keeps unknown metadata/consumer scope
unresolved. Positive consumers remain pending even when the overall process scan
is incomplete. Equal numeric versions with trailing zero components compare
equally; decorated analyzer versions are accepted only for Rust. A newer compiler
cohort does not authorize changing the adopted project toolchain.

Discovery and planning use one parsed catalogue snapshot, validated before any
metadata client starts. Unsupported metadata URLs yield per-package unresolved
results without exposing their contents. A missing package can remain
`install-required` while its release is unresolved; this names the unmet need,
not permission or readiness to install. `stage-compatible-update` likewise names
a staging operation requiring subsequent runtime acceptance. The
[transport](../../crates/harness-core/src/dependency_fetch.rs)
selects `curl.exe` through the Windows system directory API, never through PATH.
The installed client observed here is curl 8.21.0 with Schannel. It requires at
least 8.4.0, when [curl began enforcing the size limit on unknown-length responses](https://curl.se/docs/manpage.html#--max-filesize).
Each request has a 16 MiB body limit, a 45-second transfer timeout and a 50-second
management-job deadline. No decompression is requested. The current orchestration
limits concurrency to two clients.

Only the six exact current catalogue metadata URLs are accepted. Redirects are
rejected, `.curlrc` is disabled, and inherited TLS key-log/trust-override variables
are removed. Normal network proxy configuration remains available. Raw failures
and response bodies remain in private temporary output and do not enter error
reports. Metadata requests install no package and change no project toolchain.
The upstream schemas are the [PyPI JSON API](https://docs.pypi.org/api/json/) and
[npm registry API](https://github.com/npm/registry/blob/main/docs/REGISTRY-API.md),
plus the Rust stable channel manifest.

`dependency-metadata-client-failures-second.stdout` records all six transport
tests passing, including the separately enabled real system-client cases, in
0.96 seconds after a 3.53-second compile. An owned loopback server exercises a
successful response, HTTP error, unfollowed redirect, unknown-length size overflow,
TLS failure and deadline. Its HTTP override is test-private; production exposes
no custom endpoint or client switch. The preceding failed fixture run identified
that accepted Windows sockets inherited nonblocking mode; the fixture now sets
blocking mode before reading, with a finite read timeout.

`dependency-plan-second-unit.stdout` records all 11 planner tests passing. Parent
review corrected unsupported-source handling, decorated version admission and
positive-consumer preservation. The first test run also exposed two bad test
expectations: missing metadata precedes consumer selection, and the approved
`basedpyright` URL legitimately contains the substring `pyright`. The corrected
oracles retain failure-state and output-field checks.

`dependency-plan-first-cli.stdout` records eight default CLI tests passing in
14.25 seconds after a 31.72-second compile. The separately enabled official
metadata case passed in 7.88 seconds (`dependency-plan-actual-cli.stdout`),
fetching all six releases while keeping an absent owned dependency home absent.
Formatting and both packages' all-target Clippy passed; the initial formatting
check reported only the newly expanded help line before it was formatted.

The actual outside-checkout command used the rebuilt debug manager with
`--source D:/home/sergey-akhalkov/codex-harness --user-home C:/Users/noilw
--probe-versions --processes`. `dependency-plan-actual-installed.json` records:

| Dependency | Installed | Official metadata | Proposed action |
| --- | --- | --- | --- |
| Serena | 1.7.0 | 1.7.0 | Reuse |
| Codebase Memory | 0.10.8 | 0.10.8 | Reuse |
| Graphify | 0.9.55 | 0.9.57 | Preserve; consumer scope unverified |
| Nuphus | 0.2.2 | 0.2.2 | Preserve modified installation for audit |
| BasedPyright | 1.39.10 | 1.40.0 | Preserve; consumer scope unverified |
| Rust analyzer cohort | 1.97.1 | 1.98.1 | Reuse under project-toolchain policy |

All six metadata observations were checked; stderr was empty. This command did
not update dependencies or establish the unverified releases as compatible.
The complete native dependency lifecycle, package staging/auditing, real MCP
operations and global activation remain separate requirements.

`dependency-plan-combined-core.stdout` records the final combined core suite:
227 passed, zero failed, 30 explicit cases ignored, 78.08 seconds. The explicit
system-client and public-metadata cases above supplement this default run.
`dependency-release-acceptance.json` retains exact commands, source/build hashes,
evidence names and scope limits. The global script registration is still active;
task 5.2 and the migration's delivery section own its replacement and activation.

Task 5.2's [archive reader work](rust-dependency-archives.md) follows this accepted
checkpoint and remains unfinished until a bounded actual lifecycle consumer,
auditing, staging and transactional publication are connected and checked.
