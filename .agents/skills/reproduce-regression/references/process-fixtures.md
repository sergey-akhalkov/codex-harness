# Process fixtures and optional Windows helper

Read only for a reproduction that launches processes. Use the consuming project's existing helpers when they satisfy the case; keep fault injection in owned isolated state.

## Fixture contract

- Allocate a unique temporary root and test-owned process tree. Allocate ports through the project's collision-safe mechanism (prefer OS-assigned ports); prove readiness belongs to this case. Avoid shared caches or live service endpoints when they would change other work.
- Capture stdout and stderr concurrently, with bounded retained output and an explicit truncation/output-limit result. Sequential full reads of the two pipes can deadlock when both fill.
- Use an observed readiness signal tied to this instance: protocol handshake, health response or a new owned receipt. A sleep or mere process existence is not readiness. Bound readiness, execution and cleanup separately as needed.
- Preserve natural exit/exit code, readiness failure, execution timeout, forced termination, infrastructure failure and assertion outcome as separate facts. Exit 0 alone cannot prove the product assertion. A runner timeout followed by successful cleanup remains a timeout.
- Clean up only resources this attempt owns. On Windows verify resolved absolute filesystem targets stay under the owned root before removal; retain process handles/creation identities or an owned Job instead of rediscovering by name or trusting a reused PID. Never stop a shared proxy or kill all processes with a given executable name. Retain evidence before deleting owned files; leave unknown ownership untouched and report incomplete cleanup.

## Optional linked helper

During the kit's Rust migration, an explicitly prepared native candidate provides
`harness-observe.exe`. Invoke its verified absolute path with `--cwd DIR
--timeout SECONDS [--ready-timeout SECONDS] [--output-limit BYTES] [--stdin FILE]
-- ABSOLUTE_EXE ARGUMENTS`. It directly uses Windows Jobs, with the same receipt
and readiness contract below. An optional `--root` must name a new absolute
directory; existing roots and redirected receipt writes are refused. Missing
receipt persistence is an infrastructure failure even when the child exited
zero. Global native command registration remains pending, so do not assume the
native helper is already on PATH. Existing linked scripts remain transitional.

[scripts/process_case.py](../scripts/process_case.py) wraps [scripts/observe.ps1](../scripts/observe.ps1) and reuses the kit's existing Windows Job supervisor. Use it only when the native test helper is insufficient and the installed linked resources are available. It requires Windows, Python, PowerShell 7.4+ on PATH, and the authoritative kit tree containing `tools/opencodex-process.ps1`; copying the Python file alone is insufficient. Read the current helper docstring/API before use, since these resources may be updated together.

The Python API is:

```python
run_case(argv, cwd, timeout=10, ready_timeout=None, output_limit=16 * 1024 * 1024)
```

Pass an absolute executable path as `argv[0]`, an explicit cwd and separately tokenized arguments; the helper does not perform shell expansion or command lookup for the child. `timeout` is 1–600 seconds; a supplied `ready_timeout` is 1–`timeout`; `output_limit` must be positive. The current supervisor request sets a 512 MiB memory limit. Choose a native alternative if that envelope is unsuitable; do not interpret a resource-limited run as an unqualified product failure.

For CLI use, invoke the resolved Python interpreter with the helper path and `--cwd <owned-project-root> --timeout 10 --ready-timeout 3 -- <absolute-child-executable> <arguments>`. Omit readiness checking if the child has no compatible receipt and the case does not require readiness; a protocol case still needs its own handshake assertion. The CLI currently exposes no `--output-limit` flag; use the Python API for that parameter.

The child receives `PROCESS_CASE_ROOT`, a newly allocated temporary directory. With readiness checking enabled, the child writes `ready.txt` there containing `READY` (the observer trims whitespace and compares case-sensitively) only after reaching the case's real readiness condition. An arbitrary prewritten receipt would invalidate that evidence. Ports are not allocated by this helper.

The returned mapping includes `status`, `ready`, `native`, `error` where available, `root`, `elapsed_seconds`, and `streams` with log paths/byte counts. The root retains request, observer/supervisor receipts, stdout/stderr and `report.json`. Inspect status before optional native fields: setup failure can leave them absent/null. Treat helper exceptions or missing receipts as incomplete infrastructure evidence, preserving any root/logs that exist.

The CLI exits zero only for `status == "exited"` and native `ExitCode == 0`; other results exit nonzero. The structured receipt is needed to distinguish a natural nonzero exit from readiness failure, output-limit termination or timeout. Product assertions belong to the consuming regression test and are additional to this launcher result.

Output is logged concurrently by the existing supervisor; the observer checks per-stream byte limits while running. This is a polled threshold, not a guaranteed exact disk-size cap. Job cleanup belongs to the supervisor; case files are retained for inspection, not automatically deleted by the Python wrapper. Verify cleanup evidence before claiming descendant termination. The helper is a process fixture, not a portable runner for arbitrary platforms or a replacement for the consuming project's regression oracle.
