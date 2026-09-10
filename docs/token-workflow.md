# Token workflow

RTK and native Code Mode are globally connected. The accepted process also
includes task-based Serena/selected-graph navigation, `apply_patch`, reuse of current results
and reasoning matched to task complexity. Already running sessions need a new
start to discover the changed configuration.

RTK is the explicit exception to ordinary hooks-off. For bulky output the agent
calls, for example, `harness-rtk.exe exec git log -n 80`; the selected
`PreToolUse` changes only the adapter mode to `compact`. The native program
runs once with actual arguments, cwd and environment; only stdout is filtered.
Stderr and exit code are preserved. CLI 0.153.4 does not pass the selected shell
to the hook, so ordinary commands are not rewritten. Details remain available
through `rtk raw`; a rerun is not required. Ambiguous shell syntax, unsupported
or machine formats and `HARNESS_RTK_DISABLE=1` bypass compression.
`global/hooks.json` and the former diagnostic handler stay inactive; the RTK
definition lives separately in `global/rtk-hooks.json`.

Detailed use is in the portable
[token-efficient-workflow skill](../.agents/skills/token-efficient-workflow/SKILL.md).
It loads for substantial output, repeated reads or effort selection. Code Mode
can process intermediate results before they enter context; it does not justify
dropping errors, skipping coverage or reindexing unchanged code. Small edits
still use the ordinary short path.

For symbols in a known file, start with Serena. Use the selected graph for
compact discovery and relationships when it avoids several reads; do not call
both tools as a checklist. Start with five graph matches, one-hop traversal,
4 KiB per delivered answer and 8 KiB initial retrieval for a small question.
Explicit expansion requires a missing fact; a single response stays within
16 KiB. Until enforced by a managed adapter, retain bulky raw responses in Code
Mode and return selected evidence plus errors, coverage and detail access.
The [portable recipes](../.agents/skills/token-efficient-workflow/references/code-retrieval.md)
own the detailed limits and traps. [CodeGraph calibration](../openspec/changes/replace-cbm-with-codegraph/design.md#calibration)
records actual comparisons and pending replacement acceptance. Automatic
CodeGraph refresh is accepted within the resource policy, but the replacement
has not changed the live CBM registration.

For a new task the launcher accepts `--harness-effort routine|standard|demanding`
as the first argument, mapping to native `low|high|xhigh`. An explicit native
effort or profile wins; ordinary start keeps `xhigh`. Changing effort inside an
already running turn of the current CLI is unsupported. An external app-server
client may set `turn/start.effort` for the next turn. Effort names do not prove
a savings percentage. Grok middle, Astra backup and escalation bounds remain.

Installation uses RTK 0.48.0 for Windows x64 with pinned archive and exe
SHA-256. Building the small adapter needs Cargo/Rust; sources are in
`tools/rtk-adapter`, build-identity artifacts stay outside Git under
`CODEX_HOME/harness/rtk`. Binary and hook-definition connection uses links.
Native trust is bound to the actual definition; there is no global trust bypass.

For first trust, launch the original CLI from `codexCommand` in
`CODEX_HOME/harness/installation.json` without `--profile`, then confirm the
single `harness-rtk.exe hook` definition in review or `/hooks`. That lets the
native editor store the machine hash in the base config for consumers that do
not select a profile. Ordinary harness startup uses this same local writer;
shared defaults are read separately by the launcher. Acceptance script
`tests/token-workflow-native.ps1 -TrustHook` uses the base consumer and checks
actual `trustStatus` with a new connection.

```powershell
./install.ps1 -TokenWorkflowOnly             # after core installation
./install.ps1 -TokenWorkflowOnly -Mode Check
./install.ps1 -TokenWorkflowOnly -Mode Update
./install.ps1 -TokenWorkflowOnly -Mode Disconnect
./install.ps1 -TokenWorkflowOnly -Mode Recover # if an unfinished transaction remains
```

Ordinary full installation includes the selected capability. Fresh `-CoreOnly`
leaves hooks off. Repeat installation preserves a manual native disable; after
such a disable, `codex features enable hooks` can restore the accepted exception.
Disconnecting the component removes its links and does not restore old
diagnostic hooks. Shared external packages and output records are not deleted
automatically. Code Mode that was enabled before the component remains on
disconnect.

Measured output-byte reductions on supported commands (git log 80 commits
97.45%, rg 200 matches 94.68%, compact git status 0% passthrough) are byte
counts, not tokenizer or weekly-quota measurements. Additional native hook
dispatch of about 250–313 ms was accepted in the measured setup. No
end-to-end raw-versus-optimized model-task speedup is claimed.

Serena/CBM retrieval on an owned fixture needed 2728 B of tool responses against
4204 B of source. After both indexes, coverage reported `metadata_changed` with
`generation_matches=true`; that is not a coverage miss and not verified
filesystem freshness. Current-source oracle or snippet evidence remains the
fallback.

Runtime contracts: [RTK 0.48.0](https://github.com/rtk-ai/rtk/releases/tag/v0.48.0),
[native hooks](https://learn.chatgpt.com/docs/hooks),
[Codex configuration](https://learn.chatgpt.com/docs/config-file/config-reference).
RTK claims of 60–90% apply to supported output. `rtk gain` estimates tokens
from text volume; it is not an observation of whole-session or weekly spend.
