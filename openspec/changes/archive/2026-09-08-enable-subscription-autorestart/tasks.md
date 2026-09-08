## 1. Recovery policy

- [x] 1.1 Configure bounded scheduler startup recovery in reusable task generation; verify three attempts and one-minute interval with deterministic task assertions.
- [x] 1.2 Add ConfigureRestart with journaled ownership-safe live update and recovery; verify preview, idempotence, no runtime mutations, rollback and conflict/pending guards in isolated checks.
- [x] 1.3 Handle actual runtime exits in the existing foreground host with at most three one-minute retries; verify successful recovery, exhaustion, immediate ownership-error failure and private logs in deterministic and native process checks.

## 2. Integration and delivery

- [x] 2.1 Exercise actual runtime crash recovery under the scheduled host, readiness-gated role restoration, preserved resource limits, retry exhaustion and intentional disconnect using owned isolated targets; retain results and cleanup evidence.
- [x] 2.2 Apply the policy globally without stopping the running proxy; verify the same process stays ready and exact Grok middle returns a tool-derived result through the installed launcher outside the checkout.
- [x] 2.3 Record the user's decision, operational recovery/limitations and verification evidence; check affected documentation links, PowerShell syntax and strict OpenSpec validation before closing all tasks.
