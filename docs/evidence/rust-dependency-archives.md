# Native dependency archives, audit and candidate preparation

2026-09-10, Windows x64. This is an unfinished part of
[migration task 5.2](../../openspec/changes/migrate-harness-to-rust/tasks.md).
The [core reader](../../crates/harness-core/src/dependency_archive.rs) checks SRI
digests and visits npm tar/gzip or ZIP regular-file content. The native
[audit consumer](../../crates/harness-core/src/dependency_audit.rs) now compares
selected installed npm packages with their exact official tar releases through
`codex-harness.exe dependencies audit --package-root DIRECTORY`. This is a
read-only lifecycle increment. Candidate preparation is being verified below;
publication and global activation remain unfinished. The audit executes no
package code and preserves installed files.

The Cargo lock adds ten packages for Base64 0.22.1, Flate2 1.1.9 with its Rust
backend, [Tar 0.4.46](https://docs.rs/tar/0.4.46/tar/) and
[ZIP 8.6.0](https://docs.rs/zip/8.6.0/zip/) with only Flate2 deflate support.
The installed crate manifests declare MSRV 1.63 for Tar and 1.88 for ZIP, within
the workspace's 1.89 requirement. Existing locked dependencies were retained.
These are third-party libraries, not relocated first-party helper programs.

SRI accepts SHA-256/384/512 and requires a matching strongest advertised digest.
This proves consistency with the supplied digest; the caller must first establish
official metadata identity. Compressed input is capped at 128 MiB, individual
npm tar files at 128 MiB, ZIP files and declared total payload at 512 MiB, and
entries at 16,384. The ZIP bound accommodates Codebase Memory 0.10.8's
296,140,288-byte executable; payload writes stream within the bounded worker. Names
are UTF-8, bounded to 4096 bytes, and reject traversal, separators/ADS/device
names, controls and trailing whitespace/dots. npm paths must start with
`package/`. Case-lookalike rejection is deliberately conservative; it is not a
claim about NTFS object identity or a substitute for guarded publication.
The Unicode whitespace rule is similarly conservative: the review's .NET path
observation does not establish aliasing for native parent-relative opens.

Tar's raw first pass bounds extension payloads before the library collects GNU
long names or local PAX fields. Links, special entries and oversized extension
bodies are rejected. After tar's first terminator, only zero padding is accepted;
the buffered gzip decoder must consume exactly one complete member and all
compressed input. ZIP preflight walks the selected central directory's exact raw
record count and compares it with the library's deduplicated index before any
callback. It checks selected footer/count/central directory bounds before index
creation; the current reader rejects ZIP64,
multi-disk, overlapping payloads and trailing data. It accepts stored/deflated
files and rejects encrypted or nonregular entries. Full payload/trailer reads
check CRC/size failures, including files skipped by callbacks. A read failure
stays failed even if a callback ignores its first error.

Callbacks receive tentative data and must use an owned staging scope. A later
entry or trailer can invalidate earlier callbacks. The audit verifies official
archive identity before parsing and runs in the existing bounded management job;
it emits a report only after full archive success. Future staging must discard
failed tentative work and publish only through the native ownership/transaction
boundary. In particular, the third-party ZIP
parser can search alternative footer candidates; the admission limits alone are
not evidence of an aggregate process-memory or deadline guarantee. No claim is
made that library validation replaces OS containment.

Local logs use the core lifecycle evidence directory described in
[core installation](rust-core-installation.md), prefix `dependency-archive-`:

- `fetch.stderr`: explicit Cargo acquisition and lock update, ten added packages.
- `first-check.stderr`: native core compile passed in 8.08 seconds.
- `first-unit.stdout`: seven cases passed and two failed. Those two exposed an
  incorrect assumption that Windows ordinal comparison equates long-s and S on
  this host. The reader now rejects Unicode case lookalikes conservatively without
  asserting that those names identify one NTFS object.
- `third-unit.stdout`: all nine cases passed in 0.01 seconds after a 7.13-second
  compile, including strongest-digest mismatch, path/device escapes, archive
  links, GNU names, declared size/count limits, duplicate/lookalike names,
  damaged gzip/ZIP CRC, skipped payloads and ignored callback errors.
- `second-clippy.stderr`: current core all-target Clippy passed in 5.88 seconds
  after the sticky-read-error change. `final-format.stdout` records current
  formatting success. These are helper checks, not the pending CLI acceptance.

## Explicit official npm file audit

The CLI selects only Codebase Memory, BasedPyright, the Nuphus wrapper and its
two Windows payload packages. Installed names and stable three-part versions
must validate before any network operation. The
[npm version API](https://github.com/npm/registry/blob/main/docs/REGISTRY-API.md)
response must have that exact name/version and the canonical registry tar URL.
SRI from that response is checked over the same downloaded bytes before parsing.
The in-archive `package.json` must independently have the selected identity.
This is TLS registry/digest consistency, not npm signature or provenance
attestation verification. There is no arbitrary URL or client override.

The parent starts its own manager in the existing 512 MiB, 50% CPU management
job with a 125-second deadline. Before parsing, the worker queries its immediate
job's memory, CPU and kill-on-close limits using the installed Windows API
([documented nested-job behavior](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-queryinformationjobobject));
it also has a 120-second watchdog. Nested system curl processes retain their
own transfer bounds. Worker TMP/TEMP point inside the parent's unique temporary
directory, so ordinary success/failure and worker termination share a cleanup
scope. Parent-crash orphan reconciliation belongs to the unfinished lifecycle.
Metadata is capped at 16 MiB, archives at 128 MiB and returned JSON at 1 MiB.
Worker failure output is reduced to fixed codes; raw errors and bodies are not
forwarded. No package manager, package script or model is invoked.

Each installed file is read from one handle that refuses new writer/deletion
handles. The current consumer reuses the native registration module's retained
ancestor chain and opens the leaf relative to its parent handle without following
reparse points. It rechecks the parent after the leaf pins it nonempty. This
replaces the first audit implementation's absolute open/final-path comparison;
the controlled redirected-parent test exercises the new read consumer itself.
Archive `/` separators are normalized before native component validation. The
package manifest/ancestor leases remain held through the audit. ReadGuard is
deliberately read-only and does not acquire FileGuard's TxF mutation protection;
preexisting writable mappings are not frozen. Hashes describe observed streams,
not an installation-wide transaction or runtime compatibility test. Extra
installed files are explicitly not enumerated. Reports always retain
`activation_allowed: false`; `upstream-files-match` covers only files present in
the official tar. At most 128 differences are detailed, with truthful total and
truncation counts. Installed package bytes are never changed.

Current actual CLI evidence, from outside the checkout, uses the same local
evidence directory and the `dependency-audit-` prefix:

- `first-check.stderr`: the first compile exposed a closure parameter inference
  error; explicit callback types corrected it.
- `first-unit.stdout`: four initial audit cases passed. `combined-unit.stdout`
  subsequently passed 39 dependency cases, with four explicit-only cases ignored,
  including archive, release selection, process observations and six audit cases.
- `second-cli.stdout`: the real official Nuphus 0.2.2 archive exercised the bounded
  CLI on an owned incomplete package: three missing files and one modified
  manifest. A real 404 for an absent release returned a fixed source-error code;
  both attempts preserved package data and left the owned temporary root empty.
- `combined-cli.stdout`: nine default discovery/audit CLI cases passed; two
  explicit HTTPS cases stayed ignored in that default run.
- `installed-codebase-memory.json`: all four official wrapper files matched
  installed Codebase Memory 0.10.8. Its separately downloaded native executable
  is outside this npm tar and is not covered by this result.
- `installed-nuphus.json`: three wrapper files matched and one differed for
  Nuphus 0.2.2; the modification was preserved.
- `installed-nuphus-native.json`: the first nested-file audit exposed the slash
  comparison defect, reporting four unavailable files. The nested-path unit
  oracle passes after correction. `installed-nuphus-native-second.json` then
  matched all five official files, 31,011,482 payload bytes, in the installed
  Windows x64 companion. No executable or DLL was run or replaced.
- `installed-basedpyright.json`: all 5419 official files matched BasedPyright
  1.39.10. This exercises a substantially larger archive/file count through the
  same actual bounded consumer; dependencies and extra files remain outside its
  claim.
- `second-clippy.stderr`: the earlier core and CLI all-target Clippy passed in
  9.68 seconds. The earlier `final-format.stdout` records a help-line wrapping
  failure, corrected with the workspace formatter before the subsequent check.
- `relative-unit.stdout`: all six audit cases passed after switching to native
  parent-relative observation. `registration-unit.stdout` passed 30 native
  registration cases with three explicit child cases ignored, including the new
  capture-then-redirect read test. The earlier installed reports above retain
  their earlier read-boundary identity.
- `relative-cli.stdout`: both actual CLI cases passed in 4.80 seconds after a
  15.34-second compile, including official-source failure and cleanup.
  `relative-installed-nuphus.json` matched all five native companion files in
  8.59 seconds; `relative-installed-python.json` matched all 5419 BasedPyright
  files in 10.31 seconds. These are individual public-network runs, not matched
  performance comparisons. `relative-clippy.stderr` passed core/CLI all-target
  Clippy in 13.83 seconds and `relative-format.stdout` records formatting success.

## Candidate preparation in progress

The [staging consumer](../../crates/harness-core/src/dependency_stage.rs) and
`dependencies stage --package NAME --version VERSION --state DIRECTORY` are
explicit native entry points. They use native-owned state, a unique candidate directory,
official metadata/SRI and the same bounded worker. The tar visitor now has a
directory callback so staging can retain explicitly empty directories. Regular
payloads use a new streaming StagedFile constructor with a 512 MiB file limit;
the existing 16 MiB configuration-record API is unchanged. Each file commits
only within the tentative candidate. Whole-archive completion produces a file
manifest; no installation is promoted. A later archive failure invalidates the
candidate and the parent owns its cleanup.

`dependency-stage-first-check.stderr` retains the first failed combined compile
caused by missing Windows constants in the concurrent MCP-probe module. After
correction, `dependency-stage-first-unit.stdout` passed three staging cases,
including integrity/identity, empty directories, foreign files and transacted
20 MiB writes. `dependency-stage-first-cli.stdout` passed two CLI cases,
including official Nuphus wrapper preparation and failed-download cleanup.

Independent archive review then identified silent partial reads. The native
`dependency-archive-review-repro.stdout` run reproduced all six added failures:
nonzero content after tar's first terminator, a second gzip member, trailing
compressed input, duplicate ZIP names, a lying ZIP count, and admission of
trailing Unicode whitespace. The first five are reader defects; the sixth is
the conservative name-policy change described above. After correction,
`dependency-archive-review-fixed.stdout` passed 50 dependency cases (four
explicit-only cases ignored). `dependency-stage-archive-fixed-cli.stdout`
passed all four audit/staging CLI cases with public npm acquisition and 404
cleanup enabled. These runs precede the subsequent ZIP size-bound adjustment.

The first actual Codebase Memory preparation failed closed. Fixed public error
codes now distinguish native integrity, native archive, missing executable and
npm archive rejection without exposing response bodies or signed redirects.
`dependency-stage-codebase-reason.stderr` identifies native archive rejection;
the installed 0.10.8 executable exceeded the original 128 MiB file limit. The
larger streaming/ZIP bound subsequently passed actual preparation in
`dependency-stage-codebase-sized.json`: five files, 296,175,048 payload bytes.
The archive SHA-256 `b43ad982994c4d829670749e08d3b622a74bb20041fc0a7d02bef6113f81c34d`
matches the official release asset's metadata; an independent filesystem pass
matched every staged file's size and SHA-256 with the manifest. The acquisition
allows one exact GitHub release-assets CDN hop in repository id 1166102148's
namespace; signed redirect parameters never appear in the report. See the
[release API](https://docs.github.com/en/rest/releases/releases#get-a-release-by-tag-name)
and [asset API](https://docs.github.com/en/rest/releases/assets#get-a-release-asset).
Earlier audit checkpoints retain their prior source identities.

`dependency-codebase-installed-native-comparison.json` separately observed the
installed executable's SHA-256 as
`b4b403b1d7c4def3785f148b93f345ce8427858f4f5489ce28580c4387a336a6`, matching
the officially staged executable. This closes the earlier four-file npm audit's
specific native-executable provenance gap; it does not certify extra installed
files, an installation-wide snapshot or runtime compatibility.

The subsequent actual Codebase Memory MCP probe returned
`MCP private state cleanup failed; retained for recovery`, saved in
`dependency-probe-codebase-actual.stderr`. Protocol acceptance is not claimed
from that failed operation. No surviving MCP process was observed in the later
inspection, and the retained files were exclusively openable. The first operation
discarded its native error code; a transient post-exit sharing failure remains an
inference, not a proven cause. The original private tree and a separate evidence
copy were retained. The corrected cleanup uses a separate five-second retry
budget and reports whether it retried plus the initial numeric OS error when
available. The job-cleanup budget is separate. Protocol and cleanup errors are
both retained if they occur together; no foreign message or private path is
forwarded.

The new fixed-handle test explicitly denies delete sharing and handshakes with
the fixture before it continues. It confirms a failed first deletion followed
by successful bounded retry. A second test holds the file past the cleanup
budget while returning an incorrect graph result. The first parent run reproduced
the hidden cleanup failure (`dependency-probe-combined-error-repro.stdout`);
after correction it reports both the graph error and retained state. The later
`dependency-probe-parent-second.stdout` exposed that TempDir's error wrapper lost
the native error code; direct filesystem cleanup now preserves it without
printing the path. `dependency-probe-parent-third.stdout` passed all 14 probe
cases, with the manually selected installed-artifact test ignored. The preceding
compile failure is retained in `dependency-probe-parent-fixed.stderr`.

Pinned upstream review also found two relevant runtime constraints:
[README v0.10.8](https://github.com/DeusData/codebase-memory-mcp/blob/v0.10.8/README.md)
(blob `ca39ef4182aef94e1d51673a5686cf18827a8056`) documents account-wide cache-root
admission while processes are active, so a private cache may conflict with a
current global session. No such conflict was observed in the successful probes
below; it is not the diagnosed cause of the first failure.
[The MCP source](https://github.com/DeusData/codebase-memory-mcp/blob/v0.10.8/src/mcp/mcp.c)
(blob `618d04469fe544f51d8253f5f79a786b4e5b6be1`, schema at line 393 and selection
at line 8227) defaults indexing to `full`; explicit `fast` omits similarity and
semantic edges. The structural smoke probe now verifies that the schema admits
`fast` and sends it explicitly; the fixture checks the request and rejects a
missing mode contract. Neither finding
authorizes stopping global sessions or relaxing the fixture's function-query oracle.

`staged-unverified` does not certify runtime compatibility or permit activation.

Final increment checks, with commands and source/binary identity retained in the
local `dependency-stage-final-` logs and `dependency-stage-final-checkpoint.json`
(26 source inputs; manager SHA-256
`1eca420aa2424767e088f6c95bd2eb26428cb80b577b46b94ac6ca1052a665c2`):

- `unit.stdout`: 50 core dependency tests passed, four explicit-only cases
  ignored. `transport.stdout` separately passed all six transport cases,
  including the two explicit installed-curl/loopback checks; these groups overlap.
- `owner.stdout`: the owned-state/foreign-state and lock test passed.
- `cli.stdout`: all five audit/staging CLI tests passed with public npm/GitHub
  acquisition enabled, including exact native ZIP identity, streamed file hashes,
  failed-download cleanup, unsupported options and foreign-state preservation.
- `clippy.stderr`: core and CLI all-target Clippy with `-D warnings` passed.
  `format.stdout`/`format.stderr`: workspace formatting check passed.
- `dependency-probe-codebase-final.json`: actual final CLI probe passed all seven
  operations, 15 tools and exactly one owned indexed project. The fixture's
  `inventory_probe` function was queried through the real graph.
- `dependency-probe-nuphus-final.json`: actual final CLI probe confirmed all 38
  tools without desktop/browser calls. The executable and two companion DLLs
  matched official archive hashes before and after execution. Six missing
  selector/ref branch `type` fields remain reported as the original schema's
  known adapter input, rather than silently presented as repaired.

Both final actual probes stopped their owned trees and removed temporary state
without retry. Earlier actual results from the child's build remain separately
identified in `dependency-probe-codebase-retry.stdout` and
`dependency-probe-nuphus-first.json`. The first failure's original cause cannot
be inferred from these later successes. Checks were sequential on the documented
Windows x64/Rust 1.97.1 host; the actual CLI was invoked outside the checkout.
These are acceptance runs, not matched timing or subscription comparisons.

The same final CLI also prepared the official Nuphus 0.2.2 Windows x64 companion
(five files, 31,011,482 bytes) and BasedPyright 1.39.10 (5419 files, 27,616,199
bytes). `dependency-stage-final-observed-files.json` records an independent
manifest-hash, file-count, size and SHA-256 pass over both retained candidates.
Their staging reports retain `activation_allowed: false`; BasedPyright runtime
compatibility has not yet been probed by this increment.

The subsequent [candidate selection increment](rust-dependency-selection.md)
validates these retained packages through their actual runtimes, including
BasedPyright, and exercises local selection/conflict/rollback. Its source and
artifact identity are recorded separately from this staging checkpoint.

Complete provisioning and registration, interrupted promotion/rollback,
orphan recovery and global dependency delivery remain open. Protocol probing
does not promote a candidate or turn its staging receipt into activation
authority. The
previously accepted [discovery/release-planning checkpoint](rust-dependency-discovery.md)
predates these Cargo/archive changes and remains a separate source identity.
