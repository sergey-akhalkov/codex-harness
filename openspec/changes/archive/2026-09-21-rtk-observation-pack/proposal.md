## Why

Bulky supported command outputs are compressed once, but recovering detail still re-emits the whole retained raw archive into model context (`rtk raw`) or reruns the command. On long tasks this repeated replay is a measured context cost the kit already pays. The publicly documented ObservationPack mechanism from NVlabs/SoL-Pi (MIT; ideas only, no code reuse) validates that stable handles with exact paged recall cut this replay without hiding evidence. The kit already archives raw output in the RTK adapter, so the shortest path is to grow that accepted component instead of adding a parallel subsystem.

## What Changes

- Eligible `harness-rtk compact` runs emit a stable observation handle and content digest next to the existing `[rtk raw: <path>]` footer; compression, exit status and bypass behavior are unchanged.
- A new `harness-rtk recall <handle>` subcommand returns a bounded, line-windowed slice of the archived observation after digest verification, without rerunning the source command.
- Packed observations get their own bounded local retention that outlives the active session; recall reports an explicit error when a handle was evicted, never fabricates content, and never starts models or network calls.
- The token-efficient workflow skill and token workflow documentation teach recall-by-slice as the preferred detail path over whole-file raw reads.
- Adoption follows the existing benefit gate: paired fixture measurement with honest scope (bytes, turns), no session or weekly-quota savings claims.
- Attribution records NVlabs/SoL-Pi as the public inspiration. The SoL-Pi project, Pi harness, Node runtime and its code are not vendored or installed; all maintained executable code stays Rust.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `token-efficient-agent-workflow`: adds observation packing and digest-verified paged recall requirements to the existing RTK compression capability, including explicit retention, bypass and failure-path behavior.

## Impact

- Code: `tools/rtk-adapter/src/main.rs` (handle, digest, index, `recall` subcommand; one small pure-Rust digest dependency), acceptance tests in `crates/codex-harness/tests/rtk_adapter.rs`.
- Storage: new pack archive and index area under `CODEX_HOME/harness/rtk/`; existing `raw/` files and the pinned `rtk.exe` dependency are unchanged.
- Docs and skills: `docs/token-workflow.md`, `.agents/skills/token-efficient-workflow/references/tool-results.md`.
- Delivery: unchanged `--token-workflow-only` installation component, hook definition and trust flow; no new component, model route or provider dependency.