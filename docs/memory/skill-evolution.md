# Skill evolution calibration

Owning record for measurable skill-library comparison. Current requirements
live in [skill-evaluation](../../openspec/specs/skill-evaluation/spec.md) and
[autonomous-skill-evolution](../../openspec/specs/autonomous-skill-evolution/spec.md).
The closed change is
[archived](../../openspec/changes/archive/2026-09-19-autonomous-skill-evolution/tasks.md).
Private evidence roots stay outside Git.

## Pilot (task 0.1)

- Owned skill: `project-verification` (existing kit skill with native checks).
- Baseline `L`: current package (`SKILL.md` plus `references/`).
- Candidate `L'`: body shortening that keeps the same name and description;
  reference files are omitted. Fixture:
  `crates/skill-evolution/fixtures/project-verification-short/`.
- Unchanged comparison context: existing `outcome-prepare` / `outcome-run` /
  `outcome-oracle` runner, model `gpt-6-astra`, effort `xhigh`, isolated home
  containing only the compared skill.
- First case: `entrypoint` (exercise the documented product CLI).
- Budget for the first pair: one baseline run and one candidate run, 600s each.
- Isolation: `codex-harness skills isolate` plus host-frozen oracle/baseline
  files outside the writable case. The live skill package is a session marker
  and must not change.

## Observable units

Record paired runs, elapsed seconds, native oracle pass/fail, reported token
totals when the runner exposes them, and whether the skill body was actually
used. Do not invent a unified token price or a subscription-quota percentage.
Quota attribution is unknown unless a later measurement can name its source.

## Status

Isolation is implemented. The first pair ran through `outcome-prepare`,
`outcome-run` and `outcome-oracle` on installed CLI **0.155.0**.

Observed on 2026-09-18:

| Arm | Revision | Elapsed | Native oracle | Usage |
| --- | --- | --- | --- | --- |
| `L` current `project-verification` | `5a024128b180553b5c4de93410060569a3d1070dfd10afd96bdf6f253277247c` | 22s | failed | provider usage limit; token totals missing |
| `L'` shortened body, same name/description | `5294f7e3b5ffaedf6cb86e355495a1b90297a91d9952c1cdb8d7fd02397eff62` | 21s | failed | provider usage limit; token totals missing |

Both arms used model `gpt-6-astra` and effort `xhigh`. The live skill package
did not change. Control files were not writable; creating a new file in the
control directory remained possible, so the host snapshot is the integrity
backstop. Quota attribution, a unified token price and an account-allowance
percent remain unsupported. The provider limit does not authorize substituting
another model. The skill-benefit comparison is incomplete, not a reject.

The rollout error was `usage_limit_exceeded`, with retry after 2026-09-19
12:15. The next authorized pair is the gated negative case only, after that
time, with `HARNESS_SKILL_PILOT_NEXT=1`. Do not repeat the provider-limited
`entrypoint` pair.

## Pilot batch (task 0.2)

User-authorized substitute (`codex --profile xai`, model `grok-4.6`, effort
`xhigh`) ran the declared four-case shortening batch on 2026-09-18 through
`outcome-prepare` / `outcome-run` / `outcome-oracle` on CLI **0.155.0**. The
plan in source still names Astra; this batch does not rewrite that default.
The live skill package did not change. Control writes stayed denied. Private
TEMP JSON holds the rollouts. Quota attribution, a unified token price and an
account-allowance percent remain unsupported.

| Case | Role | `L` oracle | `L` time / tokens | `L'` oracle | `L'` time / tokens |
| --- | --- | --- | --- | --- | --- |
| `negative` | similar-unsuitable | failed | 65s / 229510 | passed | 80s / 233892 |
| `entrypoint` | intended | failed | 90s / 335945 | passed | 148s / 565755 |
| `missing` | boundary | failed | 86s / 238463 | passed | 81s / 236741 |
| `freshness` | held-out | failed | 163s / 467101 | failed | 128s / 352999 |

All eight arms completed. Revisions match task 0.1. Each arm used a fresh
isolated home, so cache is only within-arm `cached_input_tokens`. Order was
negative, then intended, boundary, held-out.

Selection and use: no explicit skill items. Negative arms issued no `SKILL.md`
tool commands. Intended, boundary and held-out arms used implicit `Get-Content`
of the live user `project-verification` package, including its references, not
the isolated library revision. Catalogue metadata was present. Body use is
therefore not attributable to `L` versus `L'`.

Predeclared capability `decide()`: held-out did not pass; candidate-oracle wins
on the other three cases are not an accept; intended-case tokens rose under the
candidate; live-path reads prevent attributing oracle differences to shortening.
Verdict: **inconclusive**. Not a reject of the skill and not a subscription
saving. Task 0.2 is closed on that verdict.

## Learning-cycle accounting (task 7.4)

Kernel `learning_cycle` checks record failed or inconclusive evals without
mutating the library, keep period spend across restart, block admission
overflow, treat a lost ledger as having no available budget, and skip review
when there is no signal. The Grok batch records metadata, body/use, elapsed
time, token totals, failures and within-arm cache as above. No accepted change
exists, so claimed net benefit including maintenance is not confirmed. Quota
attribution remains unknown. Inconclusive keeps this task open.

Independent add-vs-absence batch (2026-09-18, user-authorized `grok-4.6` /
`xhigh`, CLI **0.155.0**, unused case ids `cli-now` / `typo-fix` / `cli-prior`,
unique skill `harness-product-cli`): all six arms completed. Isolated child
env now overrides profile variables; `Users\\<name>\\.agents\\skills`
reads were absent. Intended and held-out **task** checks passed on both arms.
Baseline oracle failed only `negative_activation` because the model still
referenced the kit-checkout `project-verification` package, which is not that
profile path. Candidate also referenced that package and the unique skill.
Negative: absence failed `exact_edit`; the enabled unique skill passed and did
not activate. Token totals mixed; candidate was not cheaper on held-out.
Predeclared capability `decide()`: **inconclusive**. README-sufficient CLI
work cannot establish add-vs-absence benefit. Live skill package unchanged.
Gated repeat: `HARNESS_SKILL_LEARNING_CYCLE=1`.

Process probe on the unused `process` case (same gate, `grok-4.6` / `xhigh`,
600s): absence timed out without `check_process.exe` (~2.3M tokens). A first
candidate with `harness-process-check` finished in 477s, produced a checker
(`executable_check` true, descendant cleaned) but failed `positive_activation`
and `audit_history_retained`. After placing the package under a named
`.../harness-process-check/SKILL.md` path, a second candidate activated
(`positive_activation` true) and still timed out without a checker. Oracle
failed both candidate arms. No accept. Quota attribution unknown.

A later candidate finished again (`executable_check` true) and still failed
`audit_history_retained` and `positive_activation`. Repeating the process
probe does not establish an accepted change.

User decision 2026-09-19: close this change without claimed net benefit of an
accepted library change. Measurements and `inconclusive` stand. Future
candidates still use the same `decide()` rules.

A separate hooks-off live `codex exec` on CLI **0.155.0** with model
`grok-4.6` (not the Astra pilot) applied an owned skill without `$skill`.
The matching prompt did not contain the body marker; the workspace file
did, after the model read `SKILL.md`. An unrelated prompt did not write
that file. After rewriting the skill body, a later exec wrote the new
marker. Deleting the package and repeating the matching prompt timed out
without a retirement verdict. That is new-process exec, not same-turn TUI
catalogue recovery. Gated repeat:
`HARNESS_SKILL_LIVE_APPLY=1` on `skill_natural_apply`.

The same matching prompt on ordinary TUI ConPTY (hooks off, CLI 0.155.0,
`grok-4.6`) also wrote the body marker. The transcript showed a live
`SKILL.md` read for `natural-apply-probe`. That covers next-task apply
without `$skill` on ConPTY; it is not same-turn compact recovery.

A later ConPTY session on the same CLI applied `MARKER_v1`, then after an
idle wait and a disk rewrite of `SKILL.md`, the next matching prompt
applied `MARKER_v2`. An unrelated README prompt did not recreate
`applied.txt`. The probe skill requires a disk re-read of `SKILL.md` on
each use; without that, a second matching turn reused the first-turn
marker.

A ConPTY retirement session applied `MARKER_v1`, deleted the skill
package, then repeated the matching prompt. `applied.txt` was not
recreated with either marker. `skills identity --path` did not report
`delivery_complete`. That is next-turn retirement, not same-turn catalogue
omit.

A ConPTY same-turn session started with the probe skill absent, sent one
matching prompt, then wrote the skill package while that turn was in
flight. `applied.txt` received `MARKER_e7a1` before any later user
prompt. Catalogue injection is still next-turn; this path is live-file
use after a mid-turn add.

Same-turn delete of the package still let the in-flight matching turn
write `MARKER_e7a1`. `skills identity --path` did not report
`delivery_complete`. Same-turn retirement is observed through identity,
not by stopping an already loaded apply.

A ConPTY session sent an unrelated prompt, added the skill while idle,
ran `/compact`, then a matching prompt. `applied.txt` received
`MARKER_e7a1`. That is manual compact then apply, not automatic mid-turn
compact catalogue restore.

A ConPTY session with `multi_agent = true` spawned a second rollout. The
child user message asked to record the probe token without a skill name.
`applied.txt` received `MARKER_e7a1`. That is a new child without fork,
not an already-running child updated mid-turn.

A ConPTY running-child session spawned a second rollout, then wrote the
skill package while that child existed. `applied.txt` received
`MARKER_e7a1` about 30s later. That is a child applying a mid-turn add
via live file, not catalogue injection on the child's continuation.

A ConPTY resume after `[[skills.config]] enabled = false` did not rewrite
`applied.txt` on a matching prompt. `skills identity --path --codex-home`
reported `enabled` false, `delivery_complete` false, `tokens_refunded`
false. Catalogue listing of a disabled name can still happen; matching
apply did not.

A live TUI with a lowered auto-compact token limit still did not show a
`Compacting` UI. Matching apply after a padding turn is not automatic
mid-turn compact recovery. Canned ConPTY remains the evidence that auto
compact continuation omits a late catalogue add and can still read the
live `SKILL.md`.

A second live TUI with `model_auto_compact_token_limit = 800` and a 3000-
token inline prompt also showed no `Compacting` UI. Matching apply still
succeeded after that turn. Live automatic compact catalogue restore remains
unproven; canned ConPTY is still the auto-compact evidence.

A ConPTY rollback session applied `MARKER_v2`, rewrote `SKILL.md` to
`MARKER_v1`, then the next matching prompt wrote `MARKER_e7a1`. Update and
rollback are both next-turn live-file applies.

Same-turn delete with “if SKILL.md is missing, do not write applied.txt”
did not recreate `applied.txt`. Identity on the missing path was not
`delivery_complete`. Task 5.2 is closed on that live ConPTY evidence.

## Calibration for later acceptance (task 0.3)

Fixed from the first pair and example decision traces. These numbers are not a
subscription saving and are not tuned on a later acceptance sample.

- Observable units: paired runs, elapsed seconds, native oracle pass/fail,
  reported token totals when present, observed skill use. Quota attribution
  stays unknown.
- Episode limit: one baseline/candidate pair, 600s per arm, runner model/effort
  unchanged. Provider unavailability ends the episode as inconclusive.
- Period bound: not measured. Until a successful comparison exists, another pair
  requires a changed condition (provider available again) and still uses the
  episode limit of one pair. No invented daily credit.
- Catalogue baseline for the isolated pilot: one owned skill's metadata in an
  otherwise empty home. A full installed-catalogue byte budget is not yet
  measured.
- Task mix: intended `entrypoint`, negative `negative`, boundary `missing`,
  held-out `freshness`. Weights are equal until a successful comparison exists.
- Horizon `H` and a net-savings claim stay unestimated. A capability claim may
  accept only with intended + negative + held-out evidence; a cost claim needs
  its own predeclared horizon and may not borrow a capability pass.
- Estimator: predeclared case outcomes, not optional stopping and not a
  percentage. Uncertainty: missing, unmatched provider, or unresolved benefit
  is inconclusive.
- Independent acceptance batches must use cases that were not used to set these
  limits. The incomplete first pair does not close later benefit tasks and does
  not authorize repeating the same provider-limited pair.

## Publication primitive (task 1.2)

Windows publication reuses the existing harness `Registration` journal: live
source links, `ConfigChange` for the descriptor, `ConfigCreation` for new
package files, unchanged resource paths, and `recover` after an injected
in-step failure. Foreign files next to the skill slot stay in place. Only a
completed apply leaves the live link on a whole revision.

## Eval runner and isolation (task 1.3)

Reuse `outcome-prepare` / `outcome-run` / `outcome-oracle`. Control files live
outside the writable case and are not writable. Creating a new file in the
control directory can still succeed; the host snapshot is the integrity
backstop. Isolated `CODEX_HOME` cannot be the harness wrapper's live home; the
pair uses the registered upstream executable. Isolated `USERPROFILE` prevents
writes to the live user skill slot. Oracle mutation still cannot pass
independent verdict checks.

## Invocation artifacts (task 1.4)

On CLI 0.155.0, a sample rollout contains `session_meta`, `event_msg`,
`response_item`, `world_state`, `turn_context` and `token_usage_record`. There
is no dedicated skill-invocation event in that sample. Classification used by
the ledger:

- catalogue-only: a skills list without a body read; not a call
- explicit: item type `skill` or `skill_invocation`
- implicit: a completed `command_execution` or `mcp_tool_call` whose command or
  path references `SKILL.md`

Last invocation is `unknown` until coverage exists, `not_observed` when a
covered window has no explicit or implicit call, and never stored as skill
bodies or transcripts.

## Kernel traces (task 3.4)

Native checks in `crates/skill-evolution` enumerate only these controller
states: idle → reserved → accepted; reject with library unchanged; incomplete
evidence → inconclusive; period exhaustion after restart keeps spend;
catalogue overflow blocks growth. That enumeration is not a proof that a
skill text is useful or that every external dependency exists.

## Fixture consumers (task 7.1)

Two independent Git repositories used for portability checks are owned test
fixtures, not months of production use. Staging stays outside discovery and is
not auto-committed.

## Hooks-off compact (tasks 5.2–5.3)

CLI 0.155.0 ordinary TUI, hooks off: automatic compact and continuation ran;
a skill added mid-turn appeared on the next user turn and not on the first
compact continuation. Same-turn recovery remains an owning blocker. Tokens
already loaded are not treated as refunded.
The same split held with experimental context management on.

Re-checked the non-compact path on the same CLI: after a short in-flight
tool call, the same-turn continuation still omitted the late skill, and the
following user turn included it. Native catalogue refresh is next-turn, not
in-process continuation. `codex-harness skills identity` reads the live
descriptor and does not claim tokens were refunded; that read is not
same-turn catalogue injection.

Resume on the same CLI (`codex resume --last`, hooks off): a skill added
while the session was stopped appeared on the resumed first user turn, and a
skill disabled via `[[skills.config]] enabled = false` after the prior session
was still present in that resumed request. Resume is not a compact replacement
and does not currently honor disablement.

A new child without `fork_context` on the same CLI (hooks off, canned
Responses, `multi_agent = true`) received both the early and late on-disk
skills in its first sampling request. That is new-process discovery, not
same-turn parent continuation, and it does not prove an already-running child
receives a later update.

An already-running child (spawned without fork, then given a mid-turn in-flight
exec): a skill added while that child tool call was outstanding was absent from
the child's continuation and present only if a later new process started. Same
hooks-off split as the parent same-turn continuation.

Same-session next user turn (hooks off): a new skill is listed; a skill disabled
via `[[skills.config]]` after the previous turn is still listed. An unrelated
prompt still sees catalogue names but not the skill body marker.

Manual `/compact` after the turn is idle: the post-compact continuation included
a skill added while stopped. That is not the automatic mid-turn compact path,
which still omits a skill added during an in-flight tool call.

Same-session next turn after rewriting a skill description: the injected
catalogue on CLI 0.155.0 does not include those custom description strings, so
an update is not observable there. After deleting the skill directory, the next
user turn still lists the skill name. `codex-harness skills identity` remains
the hooks-off read of the live descriptor/revision.

Waiting 15s for the host skill watcher after delete still left the removed
skill name in the next user turn, while a newly added skill appeared. The
watcher does not make same-session retirement observable.

Next user turn without `$skill` can execute a filesystem read of the live
`SKILL.md`; the follow-up request contained the current body marker. That is
hooks-off application of the on-disk revision after catalogue listing, not
same-turn compact recovery and not disablement.

The same filesystem read also ran on the same-turn continuation: the catalogue
still omitted the late skill, and the exec follow-up still contained the live
body marker. Catalogue injection and live-file use are separate hooks-off
paths; only the latter works mid-turn.

The same-turn continuation also ran `codex-harness skills identity --path` on
the live package; the follow-up contained identity output and did not claim
tokens were refunded. That connects the allowed descriptor read in the current
turn without catalogue injection.

After another writer rewrote the live `SKILL.md` mid-turn, the same-turn
identity exec returned the new package revision in the follow-up. Update is
observable through the allowed read in the current turn; the injected
catalogue still does not carry description/body revisions.

After deleting the live package mid-turn, same-turn identity did not report
`delivery_complete` and did not claim tokens refunded, while the continuation
catalogue still listed the skill name. Retirement is observable through the
allowed read, not through catalogue injection.

Automatic mid-turn compact continuation still omits a late skill from the
catalogue, but a filesystem read of the live `SKILL.md` on that continuation
follow-up contained the current body marker. Compact recovery without ordinary
hooks is the allowed read, not SessionStart additionalContext.

An already-running child's continuation likewise omits a mid-turn skill from
the catalogue, while a filesystem read of the live `SKILL.md` on that
continuation follow-up contained the current body marker. Child recovery
without ordinary hooks is the same allowed read.

`codex-harness skills identity --codex-home` honors `[[skills.config]] enabled
= false`: `enabled` is false, `delivery_complete` is false, tokens are not
refunded. The injected catalogue can still list a disabled skill.

`codex-harness skills usage --codex-home` lists the same disablement as
`disabled` in the usage row.

Same-turn identity with `--codex-home` after writing `enabled = false` reported
`enabled` false and not `delivery_complete`, with case-insensitive path match
against `[[skills.config]]`. Catalogue listing of the disabled name is unchanged.

Official Codex documentation still injects compact recovery through SessionStart
`source: compact` additionalContext. That is a hook path and does not authorize
restoring ordinary hooks under this kit's hooks-off default.

## Global skill visibility

`skill-evolution` and `skills-usage-analysis` are linked from the live user
skill directory to the kit source. Two independent fixture Git repositories
outside the harness, with no project skills, listed both in the global section
of `codex-harness skills usage` and did not apply candidates. Full Check /
Recover / Disconnect of the live kit was not run for that observation.

## Lifecycle G (task 7.5)

Isolated roots: `core_install::skill_evolution_lifecycle_g_isolated_roots_preserve_project_records`
connects `skill-evolution` and `skills-usage-analysis`, adds a further skill
without a fresh install, retargets after relocation, recovers, disconnects, and
keeps a cloned project skill plus Git memory. Foreign files next to the skill
slot stay. That run is not a live-kit mutation.

Live kit (separate evidence): user skill links already point at this checkout.
`codex-harness check --core-only` on the installed manager reported an unhealthy
current source/build because this worktree is dirty, and preserved the
installation. From the harness cwd, `skills usage` lists the two skills in the
project section and not again under global. From two empty fixture Git
repositories they appear only in global. No test-only skill names were linked
globally. Disconnect of the live kit was not run.

## Requirement coverage (task 7.6)

Owning map for the change specs. Closed rows are kernel, CLI, installer or
canned CLI 0.155.0 evidence. Open rows keep the change unfinished. Private
TEMP roots stay out of Git.

| Spec requirement | Evidence | Status |
| --- | --- | --- |
| Knowledge routing, bounded maintenance, ownership, isolation, publication, promotion, consolidation/retirement, event-driven review, usage in the lifecycle, revision identity | `skill-evolution` / `skills-usage-analysis` packages; `codex-harness skills isolate`, `publish`, `identity`, `usage`; kernel and installer tests; fixture repos in tasks 7.1 and 7.7 | Mechanism accepted. Live-kit Disconnect was not run. |
| Behavioral comparison, isolated execution, decision rules, protected evaluator, episode/period budgets | `crates/skill-evolution` plan/decision/kernel; `outcome-*`; task 0.1 pair; task 0.2 Grok batch; task 0.3 calibration; task 7.4 independent add-absence batch | Rules accepted. Shortening and add-absence Grok batches are both **inconclusive**. |
| Full-cost / net benefit | Task 0.2 Grok batch; independent CLI add-absence batch; `process` absence/candidate probes; `learning_cycle` accounting | Measured. No accepted change and no claimed benefit. Quota attribution unknown. User 2026-09-19 closed task 7.4 on that limit. |
| Derived catalogue and admission budget | Task 5.1 | Accepted for owned metadata measurement. |
| Current-session activation | identity; canned TUI; live exec; live ConPTY C/D cells | Tasks 5.2 and 7.2 accepted. Catalogue injection on auto compact remains omitted; recovery is live file/identity. |
| Compact/resume recovery | Canned auto-compact continuation file read; live idle `/compact` apply; live resume disable | Task 5.3 accepted on the hooks-off live-file/identity path. Catalogue still omits a late add on automatic compact. SessionStart compact additionalContext is not installed. |
| Delegated children | Canned TUI plus live new-child and running-child apply | Live new child and running child applied the on-disk revision. Catalogue omit on a running child's continuation remains. |
| Bounded delivery / hooks-off honesty | Task 5.4; empty ordinary `hooks.json`; RTK exception unchanged | Accepted as incomplete-and-reported. Do not restore SessionStart/PostToolUse/Stop. |
| Usage ledger, project-first command, analysis skill, candidates, growth report | `skills usage`; task 7.7 | Accepted. |
| Linked-kit skill registration and running-session awareness | Isolated lifecycle G; live links in this checkout | Isolated G accepted. Running-session awareness follows the open activation/recovery rows. |

No remaining required scenario blocks this change after the 2026-09-19
narrowing of task 7.4. Task 0.2 stays inconclusive. Tasks 5.2, 5.3 and 7.2
remain closed on hooks-off file/identity plus live ConPTY apply. Commit and
push stay out of this change unless requested.
