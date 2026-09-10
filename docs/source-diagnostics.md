# Source diagnostics

From any directory in PowerShell:

```powershell
codex-harness-check.ps1 -Json
codex-harness-check.ps1 -ProjectPath <project-directory> -Json
```

From the checkout the equivalent command is
`./install.ps1 -Mode Check -Diagnose -ProjectPath <directory>`.
Without `-Json` the global command returns a PowerShell object. Status is the
`status` field, not the process exit code. Ordinary `Check` keeps installation
and protocol checks. Diagnostics do not repair found conflicts.

The native candidate exposes `codex-harness.exe diagnose` (also
`check --diagnose`) with `--project`, `--source`, `--codex-home`,
`--user-home`, `--dependency-user-home`, `--upstream`, `--profile` and
`--timeout-seconds`. Core installation registers `codex-harness-check.exe`,
which calls the same diagnostics. CLI paths resolve relative to the calling
command directory; the report is always JSON. Native port checks are recorded
in [native Rust commands](rust-native.md#diagnostics-dependencies-and-cbm).
Global cutover has not replaced the current `.ps1` entry point.

## What is returned

| Field | Meaning |
| --- | --- |
| `status` | `healthy`: finished with no findings; `attention`: conflict or intentional override; `incomplete`: a required observation is unavailable |
| `links` | Destination, expected and actual source of each managed link |
| `layers` | Configuration-layer paths, profile, active/disabled; lowest priority first |
| `settings` | Bounded declarations, computed winner, provenance and override flag |
| `skills` | Name, path, scope and enabled from native `skills/list`; same-name different files are listed separately |
| `findings` | Category, affected source and restoration action |
| `freshness` | Already running sessions and MCP/LSP remain `unknown` |

Checked settings: `model`, `model_reasoning_effort`, `approval_policy`,
`sandbox_mode`, `developer_instructions`, `features.hooks`,
`features.multi_agent`, `features.memories`. Only allowed preference values
are emitted; other values are hidden. Developer instructions report presence and
source, not text. A missing declaration is not replaced by a guessed default.
An intentional project override still requires attention, but is not a
configuration error.

## Evidence limits

Checked against CLI **0.153.4** and **0.154.0**. Its app-server accepts `cwd` and
`includeLayers` in `config/read`, but not `--profile harness` or legacy
`-c profile=...`. For ordinary harness behavior, diagnostics pass the same live
shared overrides as the launcher into the native base consumer. Explicit custom
file profiles are parsed separately through Codex and their precedence is
reconstructed. Winners of selected simple settings are labelled
`inferred-from-native-layers`. It is not a full export of the selected CLI
session's effective configuration. Base-consumer navigation and trust come from
native layers.

If a profile changes project trust, discovery or skill settings, if managed
requirements exist, or if the CLI version is not yet checked, the result is
`incomplete`. Skills are observed by the base consumer. Already running
processes, their loaded code, runtime defaults and arbitrary CLI overrides of
another launch are not checked. After source changes, a new matching consumer is
required; a healthy link does not prove that an already running server reloaded.

Contracts: [configuration precedence](https://learn.chatgpt.com/docs/config-file/config-basic#configuration-precedence),
[advanced profiles](https://learn.chatgpt.com/docs/config-file/config-advanced#profiles).

The command makes no model requests, does not run user project commands or hooks,
and does not manage existing services. Native reads may create their own runtime
caches or logs; a temporary profile home is removed after its process stops.
Raw configs, prompt bodies, skill descriptions, stderr and native parser errors
are omitted from the report. Paths and source names are diagnostic data; check
them before publishing a report.

The command is part of ordinary Install/Update/Disconnect/Recover inventory.
Do not automatically replace a damaged foreign link: establish its owner first.
For a full kit update use `install.ps1 -Mode Update`. `-CoreOnly` creates only
core inventory; on an existing installation it preserves recorded hook links
and the accepted RTK selection.
