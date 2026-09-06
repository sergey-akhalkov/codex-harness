# Graphify: selected graph and both transports

On 2026-09-06, [graphify_native.py](../../tests/graphify_native.py) exercised the existing graphifyy 0.9.44 installation through [the repository proxy](../../tools/code-tools/graphify_proxy.py). The unrelated npm Graphify package was not invoked. No package or production service was installed or changed by this check.

```powershell
& "$env:APPDATA/uv/tools/serena-agent/Scripts/python.exe" -B tests/graphify_native.py "$env:TEMP/harness-discovery.json"
```

Both the stopped-endpoint STDIO fallback and an authenticated, owned HTTP fixture passed real MCP initialization, discovery of ten upstream tools, `graph_stats` and `query_graph` with a keyword and bounded token budget. The HTTP fixture used the same installed Python module and saved graph; its access log confirmed that the proxy's requests reached that endpoint. Its process was terminated after the test.

The selected existing graph reported 88,513 nodes, 172,936 edges and 4,173 communities on both routes. Its SHA256 before and after the read-only test matched. The graph was neither replaced with a cwd-derived graph nor copied into the checkout.

`list_prs` without an explicit absolute Git worktree root is rejected before reaching upstream. Repository tools run in a dedicated STDIO process with that worktree as cwd; upstream's `gh --repo` accepts a provider repository name, so a local path must not be forwarded as that argument. No remote PR operation was part of this check.

Credentials used by the HTTP fixture were temporary environment values. The proxy can resolve the existing protected local credential or a configured credential environment variable; credentials are never written into the source or diagnostic reports.

## Identity, update and actual rollback

[graphify_identity.py](../../tests/graphify_identity.py) reproduced two different
graphs with identical node/edge counts. The proxy rejected the mismatched HTTP
listener before sending credentials and queried the selected graph through STDIO.
The matching HTTP listener was reused. Its PID/creation identity, adopted Python,
module and explicit graph arguments are checked. A corrupt graph returned an
upstream text error with `isError=false`; the proxy now marks it as an error and
refuses to report that server healthy.

The shared UV package was updated from 0.9.44 to **0.9.55** using
[graphify_update.py](../../tools/code-tools/graphify_update.py). A temporary
candidate passed actual STDIO/HTTP calls against the saved graph, the identity
counterexamples, and a relocated console-entry-point check. Promotion retained
the old environment, preserved the external wrappers, and included the protected
workstation manifest in the dependency transaction. Only the module version,
source length and source SHA256 changed in that manifest; its DACL was preserved.

The first promotion exposed a real incompatibility: UV's relocatable environment
had changed the Python launcher fingerprint expected by OpenCode. The standard
dependency Recover command restored the exact prior environment and manifest.
Staging now preserves the adopted Python launchers and validates their identity.
The corrected promotion passed the actual existing OpenCode validators through
[graphify-opencode.ts](../../tests/graphify-opencode.ts): module 0.9.55, Python and
saved graph identities match, and the existing remote MCP registration remains
valid. The production workstation service was stopped before these tests and was
left stopped; authenticated HTTP behavior was exercised with owned fixtures.

The graph still has 88,513 nodes / 172,936 edges and the same SHA256. The rollback
environment and credential-protected manifest snapshots remain under local
dependency lifecycle state. Global native Codex activation is a separate task.

## Host settings without OpenCode Workstation

Runtime and updater share the selection of `graph_path` from
`CODEX_HOME/harness/graphify.json`, falling back to the discovered workstation
graph. A missing workstation manifest requires no auxiliary file mutation. An
explicit `--codex-home` keeps this selection correct when dependency state is
stored elsewhere. Changes to host settings or the discovered manifest between
staging and promotion preserve both packages and require fresh staging.

When the kit selects a different graph, stage and promotion validate both graph
files and their hashes. The protected workstation manifest continues to describe
its own graph; only the package module identity is updated transactionally.

On 2026-09-06, [graphify_update_test.py](../../tests/graphify_update_test.py) passed
eight cases covering this selection, missing graph rejection before acquisition,
concurrent settings changes, manifest-free rollback, real protected manifest
preparation/swap/rollback, and a candidate incompatible with the second graph.
Package acquisition and MCP responses are substituted in those eight cases.

[graphify_update_native.py](../../tests/graphify_update_native.py) additionally
passed actual offline UV staging, native discovery and process inspection,
relocated CLI execution, MCP graph calls, promotion and dependency Recover in a
disposable user home with only `graphify.json` and an owned graph. It stages the
same cached version 0.9.55 to exercise clean-host update mechanics; it does not
replace the cross-version evidence above. The prior fixture package was restored
exactly, the graph/settings bytes stayed unchanged, and the source shared
environment's tree hash matched before and after. All temporary package trees
were removed after checking that no fixture processes remained.

```powershell
& "$env:APPDATA/uv/tools/serena-agent/Scripts/python.exe" -B tests/graphify_update_test.py
& "$env:APPDATA/uv/tools/serena-agent/Scripts/python.exe" -B tests/graphify_update_native.py --source-installation "$env:APPDATA/uv/tools/graphifyy"
```
