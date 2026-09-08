# Subscription efficiency: implementation evidence

Change: [reduce-subscription-waste](../../openspec/changes/archive/2026-09-08-reduce-subscription-waste/proposal.md).
Global selection, consumer checks and corrected rollout accounting are complete.
No weekly-quota percentage or repeatable model-effort benefit is established.
All 21 tasks are closed and the change is archived. Final verification found
all four deltas synchronized, 11 main specs valid, and 223 local links/anchors
valid across 35 scoped documents. Scoped whitespace checks passed.

## Selected implementation

Development without hooks is the durable default. Both the live base and linked
profile initially had `features.hooks = false`; the native setting is documented in
[Codex Hooks](https://learn.chatgpt.com/docs/hooks) and verified against CLI
0.153.4. The empty hook definitions remain linked for ownership and relocation.
The compatibility command returns before reading stdin, resolving an interpreter,
or opening a journal. Cached MCP hook calls return an empty result before broker
startup. The authenticated broker also handles old `hook` requests silently.
This change selects no hook exception or automatic diagnostic scope. A later
confirmed user decision separately authorizes narrow RTK hooks under
[optimize-agent-token-workflow](../../openspec/changes/archive/2026-09-08-optimize-agent-token-workflow/proposal.md).
That work is not accepted by this report and cannot restore diagnostic/Stop hooks.

The separate `harness-lsp` registration is retired, its launcher rejects explicit
startup with an actionable reason, and its installed backend registry is empty.
`global/code-tools.json` keeps previous language candidates under
`retired_language_candidates`, outside discovery and dependency planning. Python
alone remains in discovery for the retained explicit Serena operations.
Historical adapter sources and tests remain available for evidence; they are not
installed capabilities. Shared packages and other applications' settings remain
untouched. [Serena qualification](serena-efficiency.md) verifies explicit Python
symbol navigation, cross-file references and suitable edits through the installed
global registry. Its LSP backend remains, so this is not elimination of all LSP.

The core installer uses the native `features disable hooks` editor on an isolated
copy, verifies its result, checks that the live config has not changed, saves a
private backup and publishes atomically. This explicit user selection survives
failed connection transactions, Recover and Disconnect. Backups are never
automatically restored to enabled hooks. Foreign links and invalid TOML fail
without overwriting the live file. Native editing may move comments, preserving
their contents and unrelated parsed values.

## Bounded candidate assessment

All comparisons keep their task acceptance oracle fixed. New automatic candidates
remain off unless they complete every scenario, preserve defect detection and
reduce total accepted-result time, including setup, review and rework, without
unexplained usage migration. A material timing difference must exceed 10% and
0.2 seconds beyond the observed repeat spread; cold and warm costs are separate.
At most three matched pairs are permitted, stopping after two consistent results.
No new model-backed benchmark has been run for this change.

| Candidate | Reused evidence / decision |
| --- | --- |
| Old automatic hook stack | [Verified-delivery comparison](verified-delivery.md) exercised 36 calls. Median unchanged events cost 1.420/1.533 s, single-edit events 2.291/2.093 s, concurrent events 1.565/1.629 s. It proves historical findings, not benefit over hooks-off. Stop blocking violates the new contract. Rejected. |
| Separate harness diagnostics | No demonstrated important gap justifies a second provider. Its previous language tests do not prove accepted-result benefit. Retired; explicit project checks cover changed behavior. |
| Serena diagnostics/edit workflow | Explicit Python operations retained for the demonstrated semantic navigation/edit needs; no automatic scope passes. On the fixed tiny rewrite oracle native wins: warm means 0.040 s versus 0.262 s, gap 0.222 s > 0.202 s including spread; cold Serena 4.578 s, second client 0.336 s. Both arms pass with zero model calls. This rejects compulsory Serena editing, not suitable explicit use. No vendor patch/diagnostic flag is enabled. |
| Astra `high` instead of `xhigh` | Existing attribution cannot hold task difficulty and correctness constant. Default remains `xhigh`; no speculative quota conversion. |
| Forced decomposition of two short helpers | [Matched delegation evidence](agent-delegation.md#matched-implementation-and-parallel-work): both accepted, direct 135.814 s vs mixed 393.745 s; xAI token totals unknown. Supports cohesive bounded assignments, not blanket avoidance of available Grok. |
| Grok recovery | [Owning investigation](grok-reliability.md) has a verified negative native result, not an accepted provider fix. Its model-binding and empty-output safeguards are consumed. Current children use `xai/grok-4.6`/`xhigh`; no reserve, billing, model or Fast-mode substitution. |

The skill comparison in `accelerate-verified-delivery` remains a separate,
inconclusive experiment. Its fixed inputs and historical results are reused;
this change does not require further opencode-kit work or claim its completion.

## Automated input inventory

Inventory boundaries: `global/kit.psd1`, `global/code-tools.json`,
`global/hooks.json`, the linked profile/agents, `install.ps1`, the five existing
MCP adapters, and the subscription service/launcher entry points. Third-party
tool results and explicit user-requested inventories retain their native schemas.

| Interface | Ordinary/no-op; failure/change evidence | Output bound / detail |
| --- | --- | --- |
| Command hooks | Nine real command cases, including malformed payloads: exit 0, zero stdout/stderr, no journal | Zero; no diagnostic coverage claimed |
| Cached MCP / broker hooks | Twelve real MCP calls across post/Stop/child Stop and four tool types; 19 zero-startup cases include mutation, failed writer, old dirty/pending state, rename, config/sibling/large input and concurrent roots | Empty content and structured object; no endpoint/backend startup |
| CBM discovery | Same actual 15-tool native catalogue, schemas retained; unknown-tool error stays explicit | Description text 22,311 → 10,071 chars under Codex's initialize-prefix rendering. Full refresh policy occurs only on `index_repository`; global mandatory freshness/coverage instructions remain |
| CBM worker failures | Nine resource fixtures: cancellation, bounds, configuration drift, admission and cleanup; stderr/progress retained privately | Concise operation/cause plus unique `cbm-failure-*.json` detail reference; no raw stderr tail in model input or automatic retry |
| Serena proxy | Notifications without request IDs do not produce responses; list-change notification only for a changed tool catalogue; explicit errors preserve request ID | Real no-op/mismatch preserves bytes; native diagnostics length limit returns a too-long notice with details retrievable; source/route mismatches remain actionable |
| Graphify adapter | Actual same-count wrong-graph rejection, selected local fallback, healthy HTTP reuse, corrupt graph rejection and explicit worktree identity | One concise fallback stderr message; no repeated policy; explicit query results remain scoped to their native contract |
| Nuphus adapter / lazy stdio | Owned browser fixture: initialization creates no browser profile, stale ref fails without changing page/processes, current ref succeeds; six lazy-worker fixtures cover no-op/admission/cancellation/identity | No automatic context; native result schema retained; stale-ref cause requests a fresh snapshot, never an automatic repeat action |
| Installer lifecycle/status | Actual core and scoped installation plus Check; default code-tools output now gives state/registration/problem, not dependency inventories | Full explicit discovery via `-Detailed`; prior four-server Check detail was 23,681 serialized chars; Python discovery is now retained separately |
| Subscription wrapper/service | Existing startup writes private entry records; normal stdout is the requested CLI output; failure gives stage and private record path | No routine model notification; finite recovery does not dispatch models or change subscription routing |
| Agent handoffs | Named middle briefs define ownership/checks and compact final output; partial artifacts inspected before corrections | Changed files, checks and unresolved issues, detailed logs on demand; empty final is an output defect, not auth/quota failure |

These are interface-specific limits. A 1000-character automatic diagnostic ceiling
does not authorize truncating native JSON fields or replacing requested query data.
Repeated explicit requests remain requests; deduplication applies to unsolicited
delivery. Disabled automatic paths are absent rather than simulated clean checks.

## Verification and private evidence

Commands run from the repository on PowerShell 7.6.5 with the installed Serena
Python 3.13.14 (`python` on PATH is a Windows Store alias). Tests run source
directly; there is no build artifact. The working tree contains prior concurrent
changes; relevant source hashes are recorded in the private consumer report.

| Command | Current observed result |
| --- | --- |
| `python -B tests/code_tools_registration_test.py` | 19 passed: migration from five owned MCP registrations to four, idempotence, native TOML editor, foreign preservation, recovery |
| `pwsh -File tests/installer.Tests.ps1 -CodexCommand <recorded-original-cli>` | Final combined source: 24 scenarios / 356 assertions passed; isolated actual install/Check/disconnect, Unicode relocation, failure recovery |
| `pwsh -File tests/activation.Tests.ps1` | 19 scenarios / 152 assertions passed, including hard process crashes and recovery |
| `pwsh -File tests/code-tools-scoped.Tests.ps1` | 56 assertions passed; no dependency/subscription writes |
| `pwsh -File tests/hook-policy.Tests.ps1` | Four native TOML semantics/idempotence cases plus unchanged invalid config passed |
| `python -B tests/subscription-efficiency.py` | 19 zero-startup, 9 silent command, 12 silent MCP cases; real CBM catalogue; native defect/correction oracle and writer exit 7 preserved |
| `python -B tests/tool-resources.py` | Nine passed; owned process fixtures, no native full index |
| `python -B tests/lazy-stdio.py` | Six passed; separate `--real` case intentionally not invoked here |
| `python -B tests/serena-shared.py` | Nine routing/transport cases passed; native qualification is separate |
| `python -B tests/serena-efficiency.py -v` | Installed global registry: real Python diagnostics/edit/reference qualification, two isolated roots, reuse, fixed native oracle; automatic candidate rejected |
| `python -B tests/graphify_identity.py <installed-registry>` | Five actual identity/fallback cases passed |
| `python -B tests/nuphus-resources.py --real --focused --registry <installed-registry> --global-config <host-config>` | Owned current/stale browser-reference checks passed, no owned descendants left |
| `pwsh -File tests/subscription-global.Tests.ps1` | Actual outside native base/profile consumption, `hooks/list` empty with no warnings/errors, four registered protocol-ready MCPs, zero harness backends |
| `python -B tests/delegation-usage.py` | 34 passed, including native shared-session/child identity, conflicting response permutations, unknown fields, inherited history and direct continuation correlation |
| `python -B tests/outcome-report.py`, `tests/outcome-suite.py`, `tests/agent-delegation-evidence.py` | 12 + 16 + 4 passed against the final reporter; no model probes |

Private evidence under `%TEMP%`: `harness-subscription-efficiency-pvvxzejh/report.json`,
`harness-subscription-global-0ad87a86a066420894787a6704497226/report.json`,
`harness-activation-83d2b9b328cc4b9daae7b55d667e1f3e`,
`serena-efficiency-ru4yygz3/report.json`,
`nuphus-resources-kze0rio5/report.json`, and `harness-waste-*.log`.
Native hook policy backups remain under `%USERPROFILE%/.codex/harness/backups`.
Raw transcripts, machine identities and credentials are not copied to Git.

Preserved failed attempts: initial tests exposed empty PowerShell byte-array
handling, outdated lifecycle expectations, an omitted CBM registry environment,
the unsupported `features list --profile` command, and unconditional generated
language rows. Each relevant mechanism/test was corrected. Using the PATH kit
wrapper instead of the recorded original CLI also failed the isolated installer
probe; the actual supported command passed. None of those attempts is counted as
successful evidence. MCP SDK warnings do not establish diagnostic coverage.

The initial empty language catalogue also prevented Serena Python startup under
its real guard. Retaining only that explicit dependency and keeping the separate
harness registry empty fixed the cause. The final native qualification uses no
registry overlay. A previous connected client correctly rejects changed runtime
identity until reconnect; current-source file reads covered that navigation gap.

## Usage and routing assessment

The [final usage report](subscription-usage.md) freezes 227 byte-matching inputs
from the previous 235-input manifest; eight original hashes were not recovered.
It observes 13,550 distinct response IDs and retains usage in 138 of 145 supplied
child threads. An independent raw-record reduction matches all five token fields
in the deduplicated response series. Thirteen referenced children remain missing.
The cumulative sum (1,412,308,818) and response series (1,623,159,147) disagree;
the reconciled total is explicitly unknown. Neither is an account quota measure.

Recognized automatic-context occurrences total 308 / 1,090,158 characters;
they include hook, child and internal-context markers, including inherited or
repeated occurrences, rather than only unique LSP findings. Eighteen distinct
resumptions correlate with recognized triggers before a new ordinary user request.
That timestamp heuristic does not establish why the account limit changed.

Effort and model groups cover different task sizes, periods, contexts and partial
histories. They do not supply a fixed-outcome `high` versus `xhigh` comparison.
The global Astra `xhigh`, Astra reserves and preferred Grok middle remain unchanged.
Useful bounded Grok work was delegated; partial artifacts were inspected after
session restoration and before a fresh assignment. A worker later returned only
a promise to poll its process. The parent retrieved the completed private scan,
reviewed and corrected the parser, ran the checks and generated the final report.
That completion defect was not treated as auth/quota failure or used to switch
providers. The owning Grok-reliability change remains separate and unfinished.

## Requirement closure

Tasks 5.1–5.5 take the specification's zero-retained-automatic-scopes branch.
The command, cached MCP and authenticated broker all return before observation,
journaling or endpoint/backend startup. Thus mutation shape, file size, stale
history, pending work, concurrent roots and timeout state cannot authorize any
automatic request. The negative matrix asserts that boundary; native Python
defect/correction and failed-writer checks prove explicit verification and original
results survive. This is deliberate absence, not simulated positive LSP delivery.

Core/native TOML tests cover fresh defaults and invalid/foreign inputs. Installer
and activation suites cover install, repeated update, repair, Unicode relocation,
disconnect, interrupted transactions and crash recovery. Scoped registration tests
cover five-to-four migration without shared package deletion. Final outside
consumption verifies the selected sources; these owned fixtures are a substitute
for another physical machine, not a claim of a second-machine install.

Main specs synchronize the durable default and conditional automatic contract,
retire pending reconciliation, and retain unrelated freshness/isolation guarantees.
The active resource and delivery-comparison deltas now preserve this selection;
their unrelated unfinished tasks remain unchanged. Historical evidence is retained.

Existing sessions can retain old MCP catalogues until reconnect/restart. Command
and broker compatibility guards suppress their hooks meanwhile. CLI/app-server
evidence establishes the tested native consumers; it is not a fresh desktop/IDE
UI acceptance claim. No host restart or other application's package update was
needed for this selection.

Concurrent-source reconciliation: the RTK change generalized the native feature
editor and made the linked profile inherit the base hook setting. With no active
RTK selection, the base remains false and the effective profile has no enabling
override. This acceptance preserves that later user decision and reruns affected
core lifecycle/native checks against the combined source: installer 24/356,
activation 19/152, four native hook-policy semantics cases plus invalid-input
preservation, and the actual outside-consumer check passed. An auxiliary review
expanded into retired-language provisioning and was closed without an accepted
review conclusion; only actual checks and the parent's scoped review are claimed.
