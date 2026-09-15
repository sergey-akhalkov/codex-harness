## 1. Launcher effort defaults

- [x] 1.1 Add the per-model effort pass to the launcher argument policy (glm-5.3 to max, grok-4.6 and Astra to xhigh; explicit effort, profile, remote and selector paths unchanged; unmapped models untouched) and verify with new launcher unit tests covering each precedence path
- [x] 1.2 Wire the machine-local and portable model fallback into the native launcher's session path and verify with an existing-home fixture test that the injected override precedes portable defaults

## 2. Lean connector and feature defaults

- [x] 2.1 Add "features" to the portable-config keys where a machine-local leaf wins, set features.apps=false in global/harness.config.toml, and verify with portable-config unit tests plus a fresh-home launch argument check

## 3. CodeGraph bounded surface and CLI maintenance

- [x] 3.1 Filter the CodeGraph catalogue tools/list to codegraph_search and codegraph_detail, update the adapter instructions and stale-state hints to point at the CLI control route, and verify with catalogue and transport tests that other names stay callable internally but are not listed
- [x] 3.2 Add the native CLI control command for deliberate index, sync and status outside model sessions and verify it against the existing owned-root test fixtures

## 4. Serena filtered surface

- [x] 4.1 Hide memory, onboarding, initial_instructions and get_current_config tools in the Serena proxy tools/list with an explicit environment escape hatch, and verify with a proxy-level test or scripted handshake against a stub worker

## 5. Graphify retirement

- [x] 5.1 Remove Graphify from the dependency catalogue and the Rust retired projection, stop its runtime bootstrap during activation, keep proxy sources and rollback routes, and verify with registration and activation tests that owned graphify statements are removed while unrelated settings survive

## 6. Principles compression and docs

- [x] 6.1 Compress global/principles-of-work.md to at most 24 KiB, preserve every global-working-principles requirement in a written mapping, remove Graphify as a live selection and add the session-economy rule; verify size, mapping and internal links
- [x] 6.2 Update owning docs and memory records (code tools, token workflow, project decisions) for the new selection, effort defaults, CLI maintenance route and Apps default, and verify local links plus doc hygiene checks

## 7. Integration and acceptance

- [x] 7.1 Run the targeted native test set (launcher, portable config, codegraph catalogue/transport, registration projection, activation) and record failures or limits explicitly
- [x] 7.2 Apply the change on this machine through the installer update, remove the locally installed GitHub plugin with the native CLI, and verify config state, feature values, registration removal and fresh-session tool surfaces through kit check paths without model-backed probes
