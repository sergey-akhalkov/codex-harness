# Design

## Context

Codex can insert app-owned user/developer messages or reasoning items between a
`function_call` and its `function_call_output`. DeepSeek's Responses endpoint
validates pairing within a contiguous tool exchange: calls from one batch must
precede their outputs, while an unrelated item between them produces a 400
`No tool output found for tool call` response. Because the rejected history is
replayed, the thread remains unusable until its request is repaired or the
provider changes.

The kit already runs an owned loopback Responses shim for xAI. The native
launcher verifies and reuses that process, passes authorization through without
storing credentials, and preserves independent sessions.

## Goals / Non-Goals

**Goals:**

- Repair DeepSeek input ordering at the request boundary for every turn that
  uses the configured `ds` profile.
- Recover already affected threads via a fresh launch, without editing session
  state.
- Keep xAI transformations, provider credentials, model routing and shim
  lifecycle unchanged.

**Non-Goals:**

- Patching the installed Codex CLI or upstream DeepSeek behavior.
- Synthesizing missing tool outputs or accepting a standalone injected output.
- Changing model selection, executor capacity or non-DeepSeek providers.

## Decisions

1. Reuse the existing owned loopback shim instead of adding a second service.
   The `/deepseek/...` path selects the DeepSeek upstream and ordering repair;
   the existing path keeps xAI behavior. This reuses port ownership, process
   retirement, health verification and credential pass-through.
2. Detect the explicit `ds` profile, and the dotted `-c` provider/model
   overrides used by managed app-servers, then replace the resolved
   `model_providers.deepseek.base_url` with
   `http://127.0.0.1:<owned-port>/deepseek/v1`. If no override is present, the
   route is prepended. Replacement preserves an `exec` prompt and keeps the
   route in the same configuration layer. The default-port shortcut used for
   xAI is insufficient because the DeepSeek profile normally points at the
   provider directly.
3. For each contiguous call run, find matching outputs by `call_id`, remove
   those outputs, and insert them immediately after the call run. Items that
   were between the calls and outputs retain their relative order after the
   outputs. Already contiguous exchanges produce no rewrite.
4. Repair only explicit call/output types and never invent an output. Missing
   outputs remain a separate provider-integrity failure rather than receiving
   fabricated tool evidence.
5. Preserve the original `GET`/`HEAD`/`POST` method through the shared forwarder
   and omit a body for bodyless methods. Codex refreshes DeepSeek metadata with
   `GET /models`; treating that as a bodyless request avoids a false `411`.

## Risks / Trade-offs

- Codex JSON is re-serialized only when a repair is needed; a valid request
  returns no replacement bytes.
- The shim is selected for the `ds` profile and for dotted provider/model
  overrides. A base-config-only DeepSeek session without either signal
  bypasses it and retains the upstream limitation.
- A process started before installation still has the direct provider URL and
  must be restarted to consume the corrected transport.

## Installed acceptance evidence

- A managed app-server launched with DeepSeek provider overrides carried
  `model_providers.deepseek.base_url=http://127.0.0.1:<port>/deepseek/v1` in
  its upstream process arguments.
- The installed shim forwarded an exact
  `function_call -> inserted message -> function_call_output` request to a
  loopback strict upstream as
  `function_call -> function_call_output -> inserted message`, preserving the
  authorization header and payload content.
- A bodyless metadata request traversed the same route as `GET` rather than
  receiving the local `411`; a synthetic unauthenticated app-server therefore
  reached the provider and received its authentication error.
- Focused native checks passed: the Responses shim, task forwarder, task
  admission, task gateway, launcher integration and affected test-target
  compilation; formatting and focused clippy passed. The live provider tool
  turn was not repeated because the transport-level rejected shape is already
  exercised through the installed artifact.
