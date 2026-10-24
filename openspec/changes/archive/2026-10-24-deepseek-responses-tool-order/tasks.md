# Tasks

## 1. DeepSeek request repair

- [x] 1.1 Add the `/deepseek` route and call/output ordering repair to the
  existing Responses shim; verify unit tests cover one interrupted exchange,
  a parallel call batch, an already valid request, and unchanged xAI routing.
- [x] 1.2 Detect `--profile ds` and managed app-server `-c` provider/model
  overrides in the native launcher and route the DeepSeek provider through the
  owned loopback endpoint; verify launcher unit tests for explicit, inline,
  app-server and unrelated profiles.

## 2. End-to-end transport

- [x] 2.1 Add a strict loopback-upstream test that sends the rejected
  call/message/output shape through the shim and observes reordered input at
  `/v1/responses`, plus a bodyless `GET /models` request; verify the same test
  rejects the unadapted shape.
- [x] 2.2 Run focused harness-core tests, formatting and lint checks for the
  changed Rust code.

## 3. Installation and recovery

- [x] 3.1 Document DeepSeek loopback routing and restart recovery in the
  existing installation and delegation guides; verify links and commands.
- [x] 3.2 Deliver through the existing global kit lifecycle, confirm a fresh
  `--profile ds` launch reaches the loopback DeepSeek route while unrelated
  configuration remains intact, and replay the saved thread's rejected
  call/message/output shape through the installed shim with a strict loopback
  upstream.
- [x] 3.3 Run strict OpenSpec validation and the repository source check, then
  reconcile task checkboxes with actual evidence.
