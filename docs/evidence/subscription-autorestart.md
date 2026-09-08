# Subscription runtime recovery — 2026-09-07

Status: verified and globally active. All six tasks in [enable-subscription-autorestart](../../openspec/changes/archive/2026-09-08-enable-subscription-autorestart/tasks.md) are complete. This record preserves unsuccessful attempts alongside accepted results.

## Baseline and scope

Source revision `dfc2d6af49f2da949b2b1b0ed549a33c62d1eb4b`, with unrelated pre-existing changes preserved. Official Codex CLI 0.153.4, pinned OpenCodex 2.44.0. The global role link was missing, subscription Check was degraded, and the owned scheduled task was stopped. The previous Bun run exited at 18:47:45Z with 0xC0000409; peak job memory 1,126,326,272 bytes was below the 2,147,483,648-byte cap. This is not a reproduced memory-limit termination; the native crash cause remains unknown.

After verifying exact task ownership, absent pending transaction and free port, the existing task was started once using its native Start/WaitReady helpers. It returned ready and restored the role link. The replacement PID was 13780. No running proxy was stopped for this restoration.

Private historical receipt: `%USERPROFILE%/.codex/harness/subscriptions/runs/20260907T164806-392bbee94a0e467a9bb209d955bcc8f8.result.json`. No credentials were copied to this report.

## Attempts retained

- Global Grok consumer, `%TEMP%/codex-subscription-consumer-beefbce725604f20a690d0c23627ae45`: parent Astra selected exact `middle` / `xai/grok-4.6`; child successfully read the unique fixture marker but omitted its tool output when answering. The final relay assertion failed. The probe brief now explicitly repeats the existing code-mode output contract; assertions are unchanged.
- Consumer `%TEMP%/codex-subscription-consumer-eaa5de6d53c548748c8eda1bdc5253e0`: timed out at 240 seconds with connection retries. This invocation used Store PowerShell. Subsequent native PowerShell `/readyz` immediately returned ready for the unchanged PID 13780. No claim that the global runtime crashed or hung follows from the Store-shell timeout.
- Controlled native consumer `%TEMP%/codex-subscription-consumer-38bf2ffd647d48c89083f3b4b6d3c364`: initial Astra request produced no assistant output or delegation before its 240-second timeout; peak job memory was 170,020,864 bytes. HTTP 426 also appeared in the earlier probe that successfully delegated, so it does not by itself identify this timeout's cause. This attempt did not reach Grok.
- Scheduler-only exhaustion `%TEMP%/codex-restart-budget-9cb21c042f6c44d5a50f5df8db3ef02a`: one recorded action, incomplete exhaustion observation, exact fixture task removed. Store PowerShell was an uncontrolled host mismatch; task exit result was not retained on this first failure.
- Controlled ordinary-PowerShell scheduler action `%TEMP%/codex-restart-budget-9f7d2b2c70684352bbe79213416dc5cb`: actual task inspection after several minutes showed State 3, LastTaskResult 7, RestartCount 3, RestartInterval PT1M and one attempt. This demonstrates why scheduler settings alone do not establish runtime recovery on this host. The [MS-TSCH protocol](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-tsch/2ff4aa5a-7bc4-449f-bbb1-27475645867f) distinguishes startup conditions/action launch failures.
- Native isolated lifecycle `%TEMP%/codex-subscription-isolated-f880e0f515f64c08b054ecae4bfa772e`: install, 120-second readiness, repeat install, explicit stop/recover and independent restart passed. ConfigureRestart failed before crash injection because native registration normalized task XML. All eight global config/auth hash comparisons passed; exact fixture task and service job were cleaned. Candidate acceptance was rejected.
- Minimal native XML reproduction `%TEMP%/codex-policy-xml-77522ddc29414b38a2dab1c6880b6d30`: unique task registered/updated and never run, then removed. Scheduler prunes/reorders default settings; reparsing its XML through TaskDefinition yields the candidate definition exactly. Ownership comparisons must distinguish this normalization from material foreign edits.
- Full isolated attempt `%TEMP%/codex-subscription-isolated-c836d53cc8f84a3e83855aa1365e4ab1`: failed during C# runner initialization, before crash recovery could be exercised. A concurrent host exhaustion attempt `%TEMP%/codex-host-recovery-9b7951daa7be45898a8d53516483141b` failed with OutOfMemoryException during Add-Type before launching its child. Subsequent native checks run sequentially; limits were not removed.
- Global migration preparation `%TEMP%/codex-autorestart-global-b83cff0a60f64e28891ad12b52e0505a`: retained task/state snapshots; its initial native `/readyz` request timed out before any policy mutation. A separately resumed interactive Codex PID 24188 has connections to the global proxy, so an ordinary reconnect must account for that active consumer.
- Isolated `%TEMP%/codex-subscription-isolated-1f79e63db2cf422a86ae332148fbbe8b`: native task reported running but no host-entry log appeared before the unchanged 90-second readiness deadline. Its wrapper was identical to the earlier fixture that reached ConfigureRestart. No retained Windows event establishes the cause. Cleanup removed the exact fixture task; all eight global hash comparisons passed. The test now records private wrapper entry and process state before cleanup.

## Global activation and consumer

Before migration, the old global runtime PID 13780 naturally exited again with 0xC0000409 after 3,156,010 ms; peak job memory was 1,047,576,576 bytes, below its 2 GiB cap. The exact owned task was stopped (State 3, LastTaskResult 1, original RestartCount 0). No fault was injected into the global runtime.

After verifying stopped process identity, exact task ownership, absent competing journals and free port, ConfigureRestart changed the owned task to 3/PT1M. Starting that stopped task loaded the new host. Host PID 29796 logged `service-attempt` with maxRetries 3; OpenCodex PID 28544 returned ready, and global subscription Check passed. Private `activation.json` and source fingerprints are in `%TEMP%/codex-autorestart-global-b83cff0a60f64e28891ad12b52e0505a`.

At 20:36:45Z, a live idempotent ConfigureRestart/preview returned changed=false and preserved ready PID 28544. Config and both authorization file hashes were unchanged. Evidence is under the same directory's `live-idempotence/configure.json`. The actual policy write happened while the previous task was already stopped; this live check establishes the no-op path, not a live legacy-policy write.

Fresh global consumer `%TEMP%/codex-subscription-consumer-0f4b4ea9e8c949ab861e6d1350c94a9b` passed outside the checkout. Parent Astra `01a07d91-fe9d-7223-867f-889396902f5d` created exactly one fresh `middle` child `01a07d92-8008-74b2-ad9b-1f272f5ba682` using exact `xai/grok-4.6`. The child read the fixture-only marker with a real successful shell call, and the parent received that marker and the actual 41-versus-42 discrepancy. The fixture remained unchanged. The outer bound was 480 seconds; the native command exited successfully, with no model substitution or custom transport override.

## Verified implementation

The corrected routing suite passed 206 assertions, including real never-run task registration, scheduler normalization, exact committed ownership, material action/principal/policy rejection, idempotence and interrupted update recovery. Only planned-to-observed comparisons use native XML normalization.

The unchanged production host entry passed eight controlled cases: immediate success, recovery, success on the final retry, exhaustion, unmarked failure, false/string retry markers and a foreign failure after a runtime failure. Private log checks reject raw exception text. Fast evidence: `%TEMP%/codex-host-recovery-0e7ec212b1f344a0826fc0ee4457c193/report.json`; the prior host failed the same recovery fixture as expected.

Native host recovery `%TEMP%/codex-host-recovery-dfe7f7b3e7de4552ab9ccf7378b3ad21` passed with two attempts and a real one-minute wait. Sequential exhaustion `%TEMP%/codex-host-recovery-4bd2b40bdf5f46818a9b2fb390c89a50` passed with four attempts, three waits of at least 60 seconds and natural nonzero exit; no timeout or infinite loop. These exercise the production host with a controlled runtime module, not an authenticated OpenCodex process. Both use ordinary Program Files PowerShell and owned job containment.

Tested host SHA-256: `E28D122CAD108815FC194287C9C39EFC5776F91B97B834101A3A66B5C243F705`.

## Full isolated acceptance

`%TEMP%/codex-subscription-isolated-e4aa73b974de4d1fafd588e396313105` passed all 53 checks in 327.548 seconds. The actual scheduled OpenCodex stayed ready for more than 120 seconds, survived repeat installation and explicit stop/recovery, and accepted a live legacy-policy migration without replacing its running PID.

Only the attested fixture Bun PID 24252 was terminated. Its role was withdrawn, then the scheduled host automatically launched ready replacement PID 28508 and republished the exact role link after 79.350 seconds, including cleanup, the real 60-second wait and startup. All five service attempts retained 2048 MiB job limits. Intentional disconnect removed the owned connections; reconnect/disconnect also passed.

The fixture task and service job were removed. All eight global configuration/authentication comparisons remained unchanged. This test used separate homes, a free loopback port, static models and blocked external networking. It verifies actual runtime recovery; authenticated Grok behavior is established separately by the global consumer above. The earlier pre-entry stall remains unexplained and is not claimed fixed by test instrumentation.

Tested routing module SHA-256: `FFB8DA5A0BB5C602F17BC343A5E8729983390EDD924C737E0859A18D2B4BFFE7`. The seven changed scripts' fingerprints are retained with the global activation evidence. Final subscription Check from `%TEMP%`, outside the repository, returned ready. Windows reboot and the underlying native crash cause were not tested or resolved.

## Verification commands

Run from the repository with ordinary `C:/Program Files/PowerShell/7/pwsh.exe`, not the Store host. Native failure injection is restricted to the owned temporary installation.

```powershell
& 'C:/Program Files/PowerShell/7/pwsh.exe' -NoLogo -NoProfile -File tests/subscription-routing.Tests.ps1
& 'C:/Program Files/PowerShell/7/pwsh.exe' -NoLogo -NoProfile -File tests/subscription-service-recovery.Tests.ps1
& 'C:/Program Files/PowerShell/7/pwsh.exe' -NoLogo -NoProfile -File tests/subscription-service-recovery.Tests.ps1 -NativeCase Recovery
& 'C:/Program Files/PowerShell/7/pwsh.exe' -NoLogo -NoProfile -File tests/subscription-service-recovery.Tests.ps1 -NativeCase Exhaustion
& 'C:/Program Files/PowerShell/7/pwsh.exe' -NoLogo -NoProfile -File tests/subscription-isolated.Tests.ps1 -RunIsolatedProbes -RunRecoveryProbes
& 'C:/Program Files/PowerShell/7/pwsh.exe' -NoLogo -NoProfile -File tests/subscription-consumer.Tests.ps1 -RunModelProbes -GrokModel xai/grok-4.6 -ParentModel gpt-6-astra -Scenario Delegation -TimeoutSeconds 480
```

Command knowledge is confirmed for the corrected routing, host, full isolated lifecycle and global consumer checks under the identities above. PowerShell parsing passed for the seven affected scripts; local documentation link targets and strict OpenSpec validation passed. Static diagnostics from unrelated concurrent work are not acceptance evidence for this change.
