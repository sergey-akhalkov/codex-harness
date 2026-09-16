# Process fixtures and optional Windows helper

Read only for a reproduction that launches processes. Use the consuming project's existing helpers when they satisfy the case; keep fault injection in owned isolated state.

## Fixture contract

- Allocate a unique temporary root and test-owned process tree. Allocate ports through the project's collision-safe mechanism (prefer OS-assigned ports); prove readiness belongs to this case. Avoid shared caches or live service endpoints when they would change other work.
- Capture stdout and stderr concurrently, with bounded retained output and an explicit truncation/output-limit result. Sequential full reads of the two pipes can deadlock when both fill.
- Use an observed readiness signal tied to this instance: protocol handshake, health response or a new owned receipt. A sleep or mere process existence is not readiness. Bound readiness, execution and cleanup separately as needed.
- Preserve natural exit/exit code, readiness failure, execution timeout, forced termination, infrastructure failure and assertion outcome as separate facts. Exit 0 alone cannot prove the product assertion. A runner timeout followed by successful cleanup remains a timeout.
- Clean up only resources this attempt owns. On Windows verify resolved absolute filesystem targets stay under the owned root before removal; retain process handles/creation identities or an owned Job instead of rediscovering by name or trusting a reused PID. Never stop a shared proxy or kill all processes with a given executable name. Retain evidence before deleting owned files; leave unknown ownership untouched and report incomplete cleanup.

## Optional linked helper

Use native `harness-observe.exe` when a verified absolute path is available.
Invoke it with `--cwd DIR --timeout SECONDS [--ready-timeout SECONDS]
[--output-limit BYTES] [--stdin FILE] -- ABSOLUTE_EXE ARGUMENTS`. It uses
Windows Jobs directly, with the receipt and readiness contract above. An
optional `--root` must name a new absolute directory; existing roots and
redirected receipt writes are refused. Missing receipt persistence is an
infrastructure failure even when the child exited zero. Global native command
registration remains pending, so do not assume the helper is already on PATH.
