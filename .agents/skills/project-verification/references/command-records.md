# Command records and continuation

Read this when saving useful command knowledge, checking freshness, or resuming another session's verification. Extend the project's existing validation documentation instead of maintaining a competing catalogue. The examples below are illustrative records, not executions or commands prescribed for every project.

## Knowledge and result are separate

| Knowledge state | Meaning | Next action |
| --- | --- | --- |
| `confirmed` | This exact command was successfully exercised under recorded conditions. | Compare relevant inputs, then execute checks for the current change. |
| `docs-only` | A current manifest, CI job or document specifies it, but no applicable successful execution is recorded. | Verify preparation and identity, then run if authorized. |
| `unknown` | Command, scope or current applicability lacks reliable evidence, including invalidated confirmation. | Resolve the named uncertainty with a narrow inspection. |
| `blocked` | A concrete prerequisite, access restriction or unavailable environment prevents the required execution. | Record cause, safe alternatives and what would unblock it. |

A known command can fail on a candidate without becoming an unknown command. Preserve `confirmed` for its historical conditions and record the candidate failure separately. When relevant command/runtime/build inputs change, mark applicability `unknown` (or `docs-only` if a current definition is established), retaining the old confirmation. A new failure does not erase earlier attempts or become a pass through a retry.

## Minimal durable record

Record only fields useful to rerun or interpret the check:

- Purpose/scope, exact executable and arguments, cwd relative to the project plus the resolved test root in execution evidence.
- Provenance: relevant instruction/manifest/CI path and key, revision or observed content identity. Distinguish documented commands from inferred suggestions.
- Preparation, runtime version/resolved path, source revision including relevant dirty changes, build mode, executable and generated-input identities.
- A cheap relevant fingerprint: command definition, lockfile, runtime identity and affected generation/build inputs, using content hashes or another reproducible identity. Name the files covered; a commit alone misses dirty source, and timestamps alone may miss copied stale outputs. Do not hash the entire checkout before every tool call.
- Knowledge state and reason; last successful execution timestamp, source/build identity, result and evidence location, or explicitly `none`.
- Current attempts and results, required checks still pending, blocker or next action. Keep private paths/logs out of portable shared instructions where inappropriate; use a safe evidence locator.

Recheck the inputs when continuing after interruption or when a manifest, lockfile, runtime, generated input, preparation or executable changes. Unchanged command knowledge avoids rediscovery, but does not certify the current change. Reuse a current-task result only if its tested inputs and scope still apply, and identify it as prior execution.

## Concrete records

Illustrative `docs/validation.md` entry in a consuming repository:

```yaml
id: focused-parser
command: [npm, run, test:parser, --, --runInBand]
cwd: packages/cli
provenance: packages/cli/package.json scripts.test:parser at revision 42ac901
preparation: npm ci from repository root using committed package-lock.json
runtime: Node 22.14.0; resolved executable recorded in evidence/run-17.json
source: 42ac901 plus parser.patch recorded with run-17
build: source tests; no generated CLI bundle exercised
fingerprint:
  covered: [packages/cli/package.json, package-lock.json, packages/cli/src/parser.ts]
  identities: evidence/run-17-inputs.json contains per-file SHA-256 values
knowledge: confirmed
last_success: 2026-09-07T10:20:00Z; run-17; exit 0; 12 parser tests passed
scope: parser unit behavior only; packaged CLI acceptance still required
history:
  - run-16; same command; parser assertion failed; evidence/run-16.json
  - run-17; corrected parser; passed; evidence/run-17.json
current_result: incomplete
pending: packaged-cli acceptance against the rebuilt bundle
handoff: verify inputs against run-17, build intended CLI, run required acceptance
```

Other states for that project's command knowledge:

| Record | State and provenance | History / current result / next action |
| --- | --- | --- |
| `npm run test:package`, cwd repo root | `docs-only`; `.github/workflows/ci.yml` job `package` and current root manifest agree; Node 22 and `npm ci` specified | Last success `none`; not run. Resolve built CLI and execute packaging checks. |
| Saved `npm run test:parser` after lockfile/runtime change | `unknown` for current applicability; run-17 remains historical | Run-17 passed on old inputs; current result not run. Inspect changed dependency/preparation contract, refresh fingerprint and execute. |
| `pwsh -File tests/installed.ps1`, cwd repo root | `blocked`; project validation guide specifies PowerShell 7.4+, which is unavailable in this test environment | Attempt 18 could not launch; last success `none`. Static inspection only; installed behavior unverified. Continue independent checks, resume on an authorized host with the prerequisite. |

An interruption record should identify the last tested revision, commands that actually finished, partial logs for the interrupted command, whether owned processes remain, and the next required check. Never infer completion from a missing exit result. A focused pass followed by failed integration leaves the overall result incomplete and both records visible.

## Stale executable example

Suppose a project's `src/cli.py` prints a newly corrected value, but its documented launcher runs `build/cli.py`. A saved source-test pass cannot validate that launcher.

1. Resolve the launcher's actual runtime and `build/cli.py`; record the source, build recipe and generated-file hashes. Invoke the documented launcher with the original trigger and unchanged expected value.
2. If source tests pass while the launcher still exhibits the defect, retain both observations. Check whether the native build copies or transforms the edited source and any embedded assets.
3. Run that build in owned test state. Reidentify the generated file, invoke the same launcher/trigger, and assert the promised output and exit behavior.
4. Run remaining required checks. Record that the first launcher run used stale output; do not substitute direct source execution for the launcher result.

If the builder is unavailable, preserve the failing launcher result and source-only evidence. Mark build-dependent acceptance blocked; a manually patched bundle is not evidence that the supported build path works.
