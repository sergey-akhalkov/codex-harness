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
without a rerun: a compressed run also prints an observation handle beside the
unchanged `[rtk raw: <path>]` locator, and
`harness-rtk.exe recall <handle> [--offset N] [--limit M]` returns a
digest-verified, bounded window of the retained output. Recover missing detail
with that window first (default 200 lines, `--limit` clamped to 2000, at most
256 KiB per response, with provenance, numbered lines and the next window) and
read the whole raw archive only when complete content is genuinely required.
Packed observations stay local and bounded to 64 entries or 128 MiB, evicted
oldest first; an unknown or evicted handle exits 2 with the rerun/raw remedy, a
digest mismatch exits 3 without content, and recall never reruns the command,
starts a model or opens the network. The mechanism follows the published
NVlabs/SoL-Pi ObservationPack design (MIT; ideas only, no SoL-Pi code, runtime
or dependency). Ambiguous shell syntax, unsupported or machine formats and
`HARNESS_RTK_DISABLE=1` bypass compression.
`global/hooks.json` and the former diagnostic handler stay inactive; the RTK
definition lives separately in `global/rtk-hooks.json`.

Detailed use is in the portable
[token-efficient-workflow skill](../.agents/skills/token-efficient-workflow/SKILL.md).
It loads when bulky results, repeated retrieval, preparation or waits materially
impede the work. Code Mode
can process intermediate results before they enter context; it does not justify
dropping errors, skipping coverage or reindexing unchanged code. Small edits
still use the ordinary short path.

For mixed tool outcomes, the skill's
[result handling](../.agents/skills/token-efficient-workflow/references/tool-results.md)
keeps errors and retained details without duplicate payloads. Its
[browser and App routes](../.agents/skills/token-efficient-workflow/references/browser-and-apps.md)
cover awaited browser steps, partial-effect recovery and connected-resource
identity. These instructions guide tool use. Native comparison retained the
verified baseline routes: no repeating speed or output benefit beyond variation
was shown, and `approval_policy=never` still rejects some MCP mutations.
Desktop and window screenshots that arrive as nested JSON/base64 text are converted
to native image blocks or owned path-only results before model context. Conversation
visibility is a controller/UI obligation, not a Nuphus screenshot loop. Character
counts and thread `tokens_used` are not weekly-quota percentages.

For symbols in a known file, start with Serena. Use the selected graph for
compact discovery and relationships when it avoids several reads; do not call
both tools as a checklist. Start with five graph matches, one-hop traversal,
4 KiB per delivered answer and 8 KiB initial retrieval for a small question.
Explicit expansion requires a missing fact; a single response stays within
16 KiB. Until enforced by a managed adapter, retain bulky raw responses in Code
Mode and return selected evidence plus errors, coverage and detail access.
The [portable recipes](../.agents/skills/token-efficient-workflow/references/code-retrieval.md)
own the detailed limits and traps. The archived [CodeGraph calibration](../openspec/changes/archive/2026-09-11-replace-cbm-with-codegraph/design.md#calibration)
records the historical comparisons that supported the later full retirement;
the managed graph registration no longer exists.

For a new task the launcher accepts `--harness-effort routine|standard|demanding`
as the first argument, mapping to native `low|high|xhigh`. An explicit native
effort or profile wins. Without an explicit selection the launcher applies
per-model defaults: `zai/glm-5.3` runs at `max`, while `xai/grok-4.6` and
the Astra family run at `xhigh`; unmapped models keep the native
configuration. Changing effort inside an
already running turn of the current CLI is unsupported. An external app-server
client may set `turn/start.effort` for the next turn. Effort names do not prove
a savings percentage. Apply the current
[model selection and recovery rules](agent-delegation.md#how-selection-works);
former named levels are migration context, not required dispatch presets.

Treat sessions as consumable context, guided by measured usage: a few marathon
threads repay their whole history every turn (single sessions measured at
hundreds of millions of input tokens), while the fixed instruction floor is
about 19-28k input tokens per session start. Start a new session for a new
topic, answer small follow-ups in a fresh or forked session with a concise
handoff, and hand children concise briefs instead of parent history. This is
advisory practice, not a scheduler.

Orchestrated work applies the same accounting instead of a second budget:

- Feedback, votes and promotion are `bd` commands. Recording, merging, counting
  and archiving make no model calls; only the lead's similarity and consequence
  judgments are model work, and one triage batch is bounded by
  `feedback_batch_limit` from `global/orchestration.toml`.
- Executor assignments run in exec-mode tabs that exit and close on completion,
  and a successor is a bounded `codex exec resume` continuation turn. Neither
  leaves a growing conversation to re-read, and neither replays uncertain
  external effects.
- Waiting uses one native watcher event instead of model-side polling, and lane
  worktrees are reset and reused after an accepted merge so the next task starts
  from a warm build cache. Both save wall-clock and spend rather than tokens
  inside a turn.
- Pacing lowers new-work concurrency, the triage batch size and the reasoning
  effort ceiling when fresh scoped observations show pressure, and leaves a
  healthy executor untouched. Unknown telemetry is neither zero nor unlimited,
  and no request count is invented as a remainder.
- The benefit gate charges an improvement with the check, coordination and
  rework time of both arms before it may become a default. Byte counts and
  thread `tokens_used` still do not convert into weekly-quota percentages, and
  an adopted gate is a delivery-time comparison, not a token-budget claim.

[Agent delegation](agent-delegation.md#improvement-loop) owns the loop,
[subscription models](subscription-models.md#orchestration-spend) the provider
side, and the `board-workflow` skill the record formats.

Installation uses RTK 0.48.0 for Windows x64 with pinned archive and exe
SHA-256. Building the small adapter needs Cargo/Rust; sources are in
`tools/rtk-adapter`, build-identity artifacts stay outside Git under
`CODEX_HOME/harness/rtk`. Native `--token-workflow-only` Install reuses or
acquires that pinned archive and a bounded adapter build, then links only
`harness/bin/rtk.exe` and `harness/bin/harness-rtk.exe`. When the recorded
original CLI is present, first connect enables Code Mode and hooks on an
ordinary base config; Disconnect restores recorded Code Mode and leaves
hooks off. Binary and hook-definition connection uses links.
Native trust is bound to the actual definition; there is no global trust bypass.

For first trust, launch the original CLI from `codexCommand` in
`CODEX_HOME/harness/installation.json` without `--profile`, then confirm the
single `harness-rtk.exe hook` definition in review or `/hooks`. That lets the
native editor store the machine hash in the base config for consumers that do
not select a profile. Ordinary harness startup uses this same local writer;
shared defaults are read separately by the launcher. The native RTK acceptance
suite (`cargo test -p codex-harness --test rtk_adapter`) and the native console
fixtures check the accepted behavior with a new connection.

```powershell
& <build>\codex-harness.exe install    --token-workflow-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>   # after core installation
& <build>\codex-harness.exe check      --token-workflow-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>
& <build>\codex-harness.exe update     --token-workflow-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>
& <build>\codex-harness.exe disconnect --token-workflow-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>
& <build>\codex-harness.exe recover    --token-workflow-only --source . --codex-home <CODEX_HOME> --user-home <USER_HOME>   # if an unfinished transaction remains
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

Packed-observation accounting in the adapter acceptance suite
(`cargo test -p codex-harness --test rtk_adapter`) measures a 3000-line fixture
observation at 483,000 B for a whole-file raw re-read against 33,405 B for one
bounded 200-line recall window, plus 152 B of added footer line per compressed
run, with both windows served after the source command was removed. Those are
output bytes and avoided reruns of a local fixture, not token or weekly-quota
measurements.

Historical Serena/CBM retrieval on an owned fixture needed 2728 B of tool
responses against 4204 B of source; the CBM side of that comparison retired
with the graph tools. Current-source oracle or snippet evidence remains the
fallback.

Runtime contracts: [RTK 0.48.0](https://github.com/rtk-ai/rtk/releases/tag/v0.48.0),
[native hooks](https://learn.chatgpt.com/docs/hooks),
[Codex configuration](https://learn.chatgpt.com/docs/config-file/config-reference).
RTK claims of 60–90% apply to supported output. `rtk gain` estimates tokens
from text volume; it is not an observation of whole-session or weekly spend.

**2026-09-15:** keep global Code Mode. On installed CLI 0.154.0, `codex exec
--disable code_mode` did not drop the JS `exec` tool while the routed OpenCodex
catalogue advertised `tool_mode: "code_mode_only"`. A same-prompt Grok retrieval
that actually used tools completed with fewer recorded input tokens on the Code
Mode catalogue than on a temporary catalogue without `tool_mode`. Historical
thread totals after the 2026-09-08 enablement also include RTK hooks and a later
model mix, so they do not isolate Code Mode. Character counts and thread
`tokens_used` remain not weekly-quota percentages.

