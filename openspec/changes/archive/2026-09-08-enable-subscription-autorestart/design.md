## Context

The existing hidden per-user Windows scheduled task runs `opencodex-service.ps1`, whose bounded job propagates nonzero runtime exit as host failure. Its original RestartCount was zero. Host cleanup restores native routing and withdraws the role; readiness handling already reconnects it on an independent task restart. The observed crash exited 0xC0000409 below the recorded job cap; its underlying native cause is unknown. Native acceptance subsequently showed that RestartOnFailure=3/PT1M did not retry a successfully launched ordinary PowerShell action exiting 7. Runtime recovery therefore belongs in the existing foreground host.

## Goals / Non-Goals

**Goals:** reuse scheduler recovery and the existing foreground host; update live settings without interrupting proxy consumers; prove recovery and ownership boundaries.

**Non-Goals:** introduce a second service supervisor, diagnose the native crash, change credentials/providers, promise uninterrupted in-flight requests or refresh already loaded agent catalogues.

## Decisions

- Use Task Scheduler RestartCount=3 and RestartInterval=PT1M for startup failures. Use a bounded retry loop in the existing foreground host for marked runtime failures: initial attempt plus three retries, each after 60 seconds. Ownership/configuration failures remain immediate errors. The host never resets its retry budget. A Windows SCM wrapper would duplicate the existing lifecycle and interactive-user authorization; no additional daemon is needed.
- Add `install.ps1 -SubscriptionsOnly -Mode ConfigureRestart` under the existing operation mutex. Update only the owned task's restart settings in place with TASK_UPDATE and TASK_IGNORE_REGISTRATION_TRIGGERS. Never call Stop/Delete/Run on this path.
- Journal exact task/state before and after separately from ordinary subscription transactions; update the committed ownership XML. Recover handles that journal without process mutations. Other activation modes refuse an unresolved policy journal.
- Keep source task generation authoritative for future Install/Update. Existing resource limits, logon startup and cleanup behavior remain shared with the tested service host.
- Extend the isolated native lifecycle probe to crash only its attested fixture runtime, observe host-managed replacement, and verify role recovery. Exercise retry exhaustion through the unchanged host entry with a controlled runtime module and real one-minute waits. Tests preserve private evidence and clean only owned task/process identities.

## Risks / Trade-offs

- A runtime crash interrupts in-flight requests → bounded retry restores later availability; document the gap and frozen session role catalogues.
- An update or recovery races another writer → exact ownership comparison, installer mutex, journal and preservation on conflict.
- Persistent faults keep failing → three attempts then visible stopped state; no claim that automatic restart fixes the underlying native crash.
- Real recovery testing could disconnect the controlling session → use separate fixture homes, task and loopback port; only read the live runtime.

## Migration Plan

Validate fixtures first, preview ConfigureRestart, record the live PID and task/state baseline, apply in place, and verify the same PID. A host already executing the old script requires one ordinary reconnect to load the new runtime loop; this is distinct from the non-disruptive settings operation and must use an independent control path after active probes finish. Verify global Check and fresh Grok delegation after activation. Retain a private pre-update task/state snapshot for rollback; pending updates use SubscriptionsOnly Recover. An intentional later disconnect removes the owned task and policy.
