## 1. Regression and correction

- [x] 1.1 Add an executor regression showing that unavailable xAI transport must fail before app-server startup; run it against the original code and retain the failure. The original control startup failed `xai_transport_failure_precedes_app_server_start` by starting the conversation without its transport manager.
- [x] 1.2 Reuse the native shim lifecycle from resolved xAI executor startup outside the app-server Job; verify the regression and existing control checks pass without touching live credentials. All 29 regular control checks passed; two opt-in native checks stayed ignored.
- [x] 1.3 Exercise real shim cold start, readiness, reuse and independent child cleanup on an owned loopback port with no provider requests. Immutable-candidate acceptance exposed a losing cold-start helper exiting between the readiness probe and process observation. Readiness is now rechecked after that exit; the corrected debug check and five installed-candidate runs passed.
- [x] 1.4 Replace inherited child spawning with the independent service bootstrap; verify temporary preflight Job cleanup and output EOF. The prior spawn reproduced a connection reset after successful preflight completion in `xai_shim_outlives_the_preflight_job_that_started_it`; the corrected real-process test passed, including output EOF.

## 2. Delivery

- [x] 2.1 Update the subscription guide and run formatting, native applicable tests, Clippy, source hygiene and OpenSpec validation. Control, transport, launcher and installation tests passed; workspace Clippy, formatting, source hygiene, ownership and strict OpenSpec checks passed.
- [x] 2.2 Deploy the verified candidate, verify its installed identity and executor probe, and exercise the installed shim from outside the checkout on an isolated port; record the historical exit-cause limitation. Deployment and the executor probe passed, recorded build inputs matched the checkout, and five real shim lifecycle runs passed outside the checkout without provider requests. The prior live shim's exact exit cause remains unknown, as recorded in the design and recovery guide.
