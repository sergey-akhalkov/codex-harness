# OpenCodex OAuth memory incident, 2026-09-06

Status: investigated; later global subscription activation completed and is archived. See the [archived tasks](../../openspec/changes/archive/2026-09-08-connect-subscription-model-routing/tasks.md).

## Observations

- At the time of the initial memory incident, the only long-running OpenCodex operation started by this installation was isolated `ocx login xai`. No proxy, scheduled startup, or Codex routing had yet been activated.
- The first browser attempt opened but saved no completed OAuth credential. A later contained browser-only attempt succeeded, as recorded below.
- The user reported roughly 10 GB of Bun memory and killed the process.
- Windows System event 2004 at **22:19:10 Moscow time** identified `bun.exe` PID 28848 with **4,817,936,384 bytes** of committed virtual memory at that snapshot. This is not a peak measurement. PowerShell Editor Services was another contributor at 984,408,064 bytes.
- No Bun process remained when inspected after the user's action. The terminated PID's command line was not captured; attribution to this login is a strong hypothesis, not a reconstructed process identity.
- SHA-256 comparison with the pre-login baseline confirmed unchanged native Codex config, harness profile, OpenCode config and OpenCode auth. Secrets were not printed or copied.

## Confirmed code defect and bounded reproduction

Installed OpenCodex **2.44.0**, npm source commit `07b48da8fd63881e848d26e0bd50087864f5573e`, bundles Bun **1.4.0**, confirmed by executing that binary inside the tested memory job. No runtime override or replacement Bun was used.

The pinned [login CLI](https://github.com/lidge-jun/opencodex/blob/07b48da8fd63881e848d26e0bd50087864f5573e/src/oauth/login-cli.ts) always provides a readline-based manual-input callback. In the [callback server](https://github.com/lidge-jun/opencodex/blob/07b48da8fd63881e848d26e0bd50087864f5573e/src/oauth/callback-server.ts), an input rejection becomes `null` and retries immediately in a loop that races each attempt against the same pending browser callback Promise. Closed readline rejects immediately. Each iteration registers more reactions on the pending Promise and queues more microtasks; browser I/O and the in-process timeout can be starved.

[The bounded regression](../../tests/subscription-oauth.Tests.mjs) reads the installed callback class, strips TypeScript under Node 24, replaces the port helper and Bun server with local fakes, and adds a mandatory **20,000-iteration cap**. Node also has a 128 MiB JS-heap limit. It uses closed readline, no network, no credentials and no Bun process.

Observed result: all 20,000 retries ran before a scheduled `setImmediate`; JS heap grew about **9.3 MB** in the second run. The cap stopped execution. Removing the manual-input callback let the fake browser callback complete and the server close. The delivered browser-only helper additionally passed checks for forced independent OAuth, absent manual input, callback-file cleanup on success/failure, and rejection of a foreign authorization destination. This proves the code defect and that the proposed path avoids it; it does not prove the killed process identity or live provider acceptance.

## Implemented and exercised containment

- Use [browser-only login](../../tools/opencodex-login.mjs) with the pinned package's own OAuth implementation and `forceLogin: true`; never share OpenCode refresh ownership.
- Place the managed runtime and its descendants inside a Windows Job Object before execution resumes. Enforce a job commit limit and kill owned descendants when the supervising handle closes. Add an external elapsed-time limit to login and administrative commands. See the [Microsoft job memory contract](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information).
- Open the browser from the supervising host, outside the job, so cleanup cannot terminate a browser started for the user.
- Exercise normal exit, memory pressure, timeout and descendant cleanup with harmless fixtures before any further Bun login. A warning-only watchdog is insufficient for this observed failure.

[The Windows integration checks](../../tests/subscription-process.Tests.ps1) passed six real, sequential Node scenarios: argument/environment/output fidelity and closed stdin; refusal to overwrite logs; timeout; descendant cleanup after normal parent exit; aggregate parent/child memory exhaustion; and cleanup after terminating only the supervising PowerShell process. The start receipt also confirms assignment to the job before execution resumes. The memory fixture bounds its own allocations independently; it does not attempt an unbounded allocation. Windows reported a peak near 129.4 MiB for a 128 MiB configured limit, so the evidence does not claim an exact byte-for-byte peak ceiling. Both fixture processes were gone after the limit event.

The delivered [PowerShell login entry](../../tools/opencodex-login.ps1) successfully completed separate xAI OAuth with closed stdin on 2026-09-06. Its job limit was 768 MiB, peak committed job memory **329,527,296 bytes** (about 314 MiB), elapsed **36,317 ms**, process exit **0**. Host-local evidence: `~/.codex/harness/runtime/subscription-probe-20260906-01/login-attempts/fd44e0f2912d4c96afa48cbf737dcaa3/result.json`. Browser callback artifacts were removed on completion. Credentials initially saved in the isolated store were subsequently moved to `~/.opencodex/auth.json`, without retaining a duplicate refresh owner.

A seventh real Windows runner scenario subsequently passed: an exception in the trusted readiness observer closes the job and terminates its owned descendants.

## Later CLI interruption

Windows event 2004 at **23:01:33 Moscow** reported another memory-pressure episode. Its listed large processes were PowerShell Editor Services (about 986 MB), Windows Terminal (about 896 MB), and Python (about 660 MB); Bun was not among them. The contained login had no surviving Bun process when work resumed. This does not identify the exact cause of Codex CLI termination.

Four old orphaned PowerShell language-server trees, dated August 30 through September 3, were identified through missing owning processes and creation-time checks, then stopped through their exact owned process handles. Their combined private memory was approximately 1.64 GB. Live Codex/MCP trees were preserved. The cleanup receipt is host-local at `subscription-probe-20260906-01/orphan-language-server-cleanup.json`.

Global proxy installation, native restoration and actual Codex model consumption still require separate acceptance evidence. The successful login and memory tests alone do not establish that delivery.

## Acceptance session disconnected, 2026-09-07

The global lifecycle probe was incorrectly launched from a Codex session using the same proxy that the probe stopped. The user reported an extended reconnect wait and restarted the session. The test had passed 45 checks, including actual repeat installation, task restart, native restoration, MCP discovery and reconnection, but its next Check failed. Its finalizer reported reconnection after Install without independently checking continued readiness. This was an acceptance-isolation defect, not successful delivery.

The last managed Bun run, PID 17264, was terminated by the Windows job memory limit after **103,385 ms**, with peak committed job memory **819,191,808 bytes** against the configured **768 MiB** allowance. The recorded result is `memory-limit`, exit **125**. Subsequent native restoration exited **0**. On resumption, the global installation reported `degraded`, the native config had no proxy base URL, and the Grok role link was absent. Disk restoration does not redirect an already running Codex client: the stranded session needed a restart.

Private evidence: `~/.codex/harness/subscriptions/runs/20260906T224457-003ffda7813d4bc6b632b60b346081e4.result.json`, matching host log `20260906T224455-9edf9353d251469488b13aca8b606d47.host.jsonl`, and `%TEMP%/codex-subscription-lifecycle-516d7e9544d74ca6a4d2167429de35b2/report.json`. These establish containment and subsequent disk recovery; they do not yet establish the allocation's cause or sustained proxy readiness.

Metadata-only correlation of `~/.opencodex/usage.jsonl` found seven successful `openai/gpt-6-astra` requests between 22:44:57 and 22:46:41 UTC. Six reported input counts between **144,850 and 182,930 tokens**; one lacked usage. Thus this run was serving substantial active traffic, not merely idling. Exact attribution of every request to the parent session is unproven. The response-state file was **24,951,184 bytes**; its conversation payload was not inspected. Allocation during request handling or state serialization is a hypothesis to test with isolated synthetic input, not a confirmed leak diagnosis. The configured 256 MiB application retention budget is not a hard bound on total Bun memory.

The disruptive global acceptance mode now refuses execution under `CODEX_THREAD_ID` or `CODEX_SESSION_ID` and its finalizer verifies Check before claiming reconnection. Disruptive follow-up used isolated homes, a separate port and its own task. The host proxy was initially kept stopped during that investigation. These are task-specific safeguards derived from this incident; the separate read-only global mode never stops or reconnects the proxy.

The [synthetic memory probe](../../tests/subscription-memory.Tests.ps1) completed 36 credential-free Responses requests with a generated 25,038,933-byte snapshot, a restart using that snapshot and three concurrent requests. Both proxy jobs exited 0, with peaks of 509,231,104 and 552,853,504 bytes under the unchanged 768 MiB limit. No external network requests occurred. Private evidence: `%TEMP%/codex-subscription-memory-7408124a95c540718731b404c1704867/`. This workload does not reproduce the actual failure: real tool-history structure differs from its flat synthetic text. Its original eight preservation checks included absence of an obsolete profile path; the final isolated lifecycle and global acceptance check the actual linked profile separately.

On 2026-09-07 the user explicitly instructed the agent to stop further investigation of this event and finish the specification, suggesting it might have been a one-off. That is the [accepted task decision](../project-decisions.md#подписки-и-модели-внешних-провайдеров), not a proven cause or claim that the event was fixed. Further reproduction stopped; the memory cap, observable failure and recovery remain in place. Final lifecycle and global delivery results are recorded in [subscription routing verification](subscription-routing-verification.md).
