# Experimental context delivery

2026-09-08, CLI 0.153.4. The accepted [Astra runtime pilot](native-context-contracts.md)
established experimental sampling and preservation of an early constraint after
automatic compaction. The setting is now enabled in the source-linked
`global/harness.config.toml`, using the existing file-profile lifecycle. This
does not enable Fast, native memories or background model calls.

Delivery evidence is under
`%LOCALAPPDATA%/codex-harness-evidence/context-install-1c61e8a4a2004307ac6eb70101e16e3b/`.
`native-profile-parse.json` records native parsing of the directly linked source
and explicit override: true/false respectively, both retaining Astra xhigh.
The native binary SHA-256 is
`444A3F0008050605CAE73CD9B7A2DCAC61294062DFAAB56DD20430FD6498518B`.
Ordinary `-p harness debug prompt-input` and the explicit false override both
started successfully from outside the checkout without a model request. Their
prompt output does not expose the experimental flag and is not runtime proof.

The attempted `app-server -p harness` probe was rejected by this CLI: file
profiles apply to runtime commands, not App Server. A fresh owned App Server
home with its base configuration linked to the profile supplied the separate
native parse/override evidence. It does not pretend to be the ordinary launch.

`tests/installer.Tests.ps1 -CodexCommand <recorded original Codex command>` passed
24 lifecycle scenarios and 356 assertions. The first invocation without that
argument selected the harness wrapper and failed inside the isolated feature
editor; its fixtures remain at
`%TEMP%/codex-installer проба f56cc480ebb84fc3be6516a8b351649f`.
Use the original command from the installation receipt for this legacy suite.

`install.ps1 -Mode Update -CoreOnly -CodexCommand <original>` completed globally;
an absolute `install.ps1 -Mode Check -CoreOnly` from `%TEMP%` returned Connected
with 19 links, 12 skills and 3 agent definitions. Base configuration, code-tools
and token-workflow registry hashes remained unchanged. The core installation
receipt was updated by its lifecycle. No subscription service was restarted.

For a new ordinary task, `codex -c
features.context_management.experimental_mode=false` is the explicit reversible
override. Removing the added table from the owned shared profile rolls back the
default. The ordinary runtime acceptance below completes task 2.2; parsing and
model-free startup alone were insufficient. Separate
[skill lifecycle acceptance](native-workflow-lifecycle.md) also passes.
The [native structured Astra consumer](rust-structured.md) subsequently passed
its real outside-checkout acceptance; it does not by itself close the context
sampling and runtime-override checks above.

## Ordinary runtime delivery and rollback

2026-09-09 local date. The Rust acceptance entry point
[`ordinary_context_delivery`](../../crates/codex-harness/tests/context_delivery.rs)
installed the existing core kit into separate owned homes and launched its
ordinary `codex.ps1` wrapper from independent directories outside this checkout.
The enabled and override homes link directly to the current shared profile.
The rollback home links to an owned detached worktree carrying that exact dirty
profile with only the context table removed. Base revision and explicit dirty
inputs are recorded in `rollback-source.json` and `rollback-runtime-inputs.json`;
line-ending conversion was corrected to match the inspected runtime bytes.

Evidence root:
`%LOCALAPPDATA%/codex-harness-evidence/context-delivery-fa8e726ea3f549b18521ee38ae280bf6/`.
The native CLI version and SHA-256 match the identity above. Each fresh TUI used
the existing ChatGPT subscription route at `127.0.0.1:10100/v1`, Astra with the
wrapper's explicit routine/low effort, and a literal confirmed `/rename` before
the single response check. This avoids the automatic auxiliary title request
observed in the earlier pilot. Credentials stayed behind an owned link to the
existing auth file. No API key, provider substitution or shared-service restart
was introduced.

| Case | Actual sampling | Result and elapsed test time |
| --- | --- | --- |
| Source-linked profile | `ContextManagement=true` | Exact `CONTEXT_DELIVERY_OK`, natural exit 0; 20.66 s |
| Explicit `-c features.context_management.experimental_mode=false` | `ContextManagement=false` | Same response, natural exit 0; 20.63 s |
| Profile table removed in owned worktree | `ContextManagement=false` | Same response, natural exit 0; 21.41 s |

Every case has one Astra sampling record, zero tool/file-work calls, an unchanged
base/profile hash and no remaining owned process. The ConPTY job imposed 512 MiB,
50% CPU, finite waits and bounded output. Requests, transcripts, sampling and
process receipts remain in each case directory. This is delivery/rollback
evidence; the earlier pilot owns preservation of the early constraint after
compaction, and neither establishes a quality or speed advantage.

`final-configuration-receipt.json` checks exact profile contents and direct link
targets after all runs. The global profile hash remains
`F0EABA8DBA0A2365FE79AC9E66043D6E430C1B353CFA72C9232EACE151908C8E`.
Separate model-free ordinary `features list` calls preserve Goals=true,
memories=false and hooks=false in all three homes. No Fast setting is added;
the actual turn contexts have no selected service tier. The CLI lists the
top-level `context_management` feature as false and `fast_mode` capability as
true even in these runs. Those rows are not evidence of the nested experimental
sampling flag or activation of Fast.

The Rust reader uses Windows' installed `winsqlite3.dll` read-only, with the SDK's
system calling convention and bounded rows/time/bytes. A retained-evidence check
accepted the prior three enabled Astra and thirteen disabled Astra records and
detected the prior non-Astra auxiliary record. Missing logs never establish a
passing model/runtime claim. `cargo fmt --all --check` and
`cargo clippy -p codex-harness --test context_delivery --offline --locked --jobs 1 -- -D warnings`
passed after the final helper changes.

Three setup attempts stopped at the native Windows trust prompt before any model
request; their evidence is retained. Matching the CLI's lowercase backslash
project-key spelling fixed the owned trust entry. The model-free setup then
passed in 9.63 s. Child PATH selection uses the already verified desktop
PowerShell fallback and does not alter global PATH. These setup failures and
corrections are part of the observed cost, without a subscription-saving claim.

To repeat an authorized case, set `HARNESS_CONTEXT_EVIDENCE` to a new absolute
evidence directory, `HARNESS_CONTEXT_MODE` to `enabled`, `override` or `rollback`,
and `HARNESS_CONTEXT_SHELL` to the installed desktop PowerShell executable, then
run `cargo test -p codex-harness --test context_delivery ordinary_context_delivery --offline --locked --jobs 1 -- --ignored --nocapture`
from this repository. Rollback additionally requires `HARNESS_CONTEXT_SOURCE`
pointing at a verified owned source with that table removed. The ignored test
makes a real subscription request and deliberately requires explicit invocation.
