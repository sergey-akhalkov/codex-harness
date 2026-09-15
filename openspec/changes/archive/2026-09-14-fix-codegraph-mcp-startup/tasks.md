## 1. Serving admission

- [x] 1.1 Split native build admission so hash-matching source-stale managers remain able to serve CodeGraph MCP and launch the owned CodeGraph broker/service, while source-consuming operations stay gated on a healthy source match. Verify with a native identity/runtime test that source-stale matching binaries are serving-admitted and that config localize/selected-build remain refused.
- [x] 1.2 Admit `mcp codegraph` and the owned CodeGraph service/create path through serving admission, and admit `mcp apply-codegraph-registration` through management admission. Verify a recorded source-stale manager with matching hashes completes MCP initialize for `mcp codegraph` and still runs Check registration.

## 2. Registration and Check

- [x] 2.1 Make owned CodeGraph Check inspect serving admission of the registered command without mutation. Verify Check reports degraded (not connected) when the command cannot handshake, and does not disable serving when binaries match after later source edits.
- [x] 2.2 Keep Install/Update from writing a CodeGraph command that later Codex startups cannot execute after ordinary source edits. Verify the registration path records a serving-admitted command and leaves unrelated MCP tables unchanged.

## 3. Evidence and delivery

- [x] 3.1 Update native MCP/runtime tests so the previous all-MCP source-stale refusal covers source-consuming runtime only, and add the reproduced handshake plus Check cases. Verify `cargo test --locked -p codex-harness --test mcp_cli --test codegraph_install --jobs 1 -- --test-threads=1` and the focused CodeGraph initialize check pass.
- [x] 3.2 Update the CodeGraph provider/installation notes with the serving-admission restart boundary: later source edits do not stop an already recorded hash-matching adapter, and native adapter changes still need explicit Install/Update plus session restart. Verify the owning docs describe that limit without private paths.
- [x] 3.3 Rebuild and re-register the installed CodeGraph command from current source, then verify a fresh Codex CLI start exposes CodeGraph with the other retained MCP servers. Use an actual initialize or tool-catalogue observation, not registration text alone.
