## Context

See [proposal](proposal.md) for motivation and [requirements](specs/global-working-principles/spec.md) for acceptance. This is a cross-cutting instruction change, so a design is required.

Inspected owners are `global/principles-of-work.md`, the project `AGENTS.md`, `docs/README.md`, `docs/memory/README.md`, `docs/project-decisions.md`, `docs/global-instructions.md`, pack skills and agent configuration. Principles already address persistence, relevant skill use, bounded retrieval and verification. The work is an audit and reconciliation, not an assumption that all existing guidance is defective.

`docs/global-instructions.md` describes a live global AGENTS link: new sessions consume source changes without another copy step. `docs/rust-native.md` states that ordinary noncompiled Markdown changes do not require compilation. Current working-tree edits overlap principles, skills, operating docs and active changes. Their starting state must be preserved.

The user explicitly prohibits modifying OpenSpec instructions because this pack does not maintain them. This agrees with the existing ownership decision. OpenSpec's own explore and update skills contain confirmation rules; they are protected external constraints, not available implementation targets.

## Goals / Non-Goals

**Goals:** Make fresh Astra consumer sessions receive concise, consistent and task-appropriate guidance, with traceable coverage of all repository documentation and instructions. Everyday acceptance includes a small correction, substantive completion, a mid-task correction or side question, and a consequential clarification.

**Non-Goals:** No OpenSpec fork, workflow override, API migration, model/provider replacement, universal new policy engine, or new evaluation framework. No claim that text editing guarantees perfect model obedience. Audit coverage is the current working tree, not Git history or previously published copies.

## Decisions

### 1. Use current official sources with an explicit coverage boundary

Seed sources retrieved during exploration on 2026-09-13:

| Source | Use and evidence status |
| --- | --- |
| [Rethinking skills and prompts for GPT-6 Astra](https://developers.openai.com/blog/rethinking-skills-and-prompts-for-gpt-6-astra) (2026-09-11) | Read article body. Precise descriptions, progressive disclosure, task-based context, proportionate tests, authority and completion. |
| [Astra model guidance](https://developers.openai.com/api/docs/guides/latest-model/gpt-6-astra) | Body reviewed, including the model-specific Markdown representation. Apply autonomy, instruction priority, writing, delegation and testing to owned prompts. API configuration and asynchronous request examples do not establish installed CLI contracts. |
| [AGENTS.md](https://learn.chatgpt.com/docs/agent-configuration/agents-md) | Global/project layering and override guidance applies to the installation guide. Fresh native inspections from two directories confirm canonical global loading and local project layering; the observed behavioral cases are recorded below. |
| [Build skills](https://learn.chatgpt.com/docs/build-skills) | Body reviewed. Concise selection metadata and on-demand bodies/resources apply to owned skills. Symlink discovery fits the existing lifecycle. Plugin distribution advice does not authorize an installation migration. |
| [Architectural visualization with Astra](https://developers.openai.com/blog/architectural-visualization-with-astra) (2026-09-04) | Body reviewed. Establish the desired experience, inspect actual outputs and incorporate feedback. Blender, Unreal and scene assets are example-specific, not required tools for this pack. |
| [Building games with Astra](https://developers.openai.com/blog/how-to-build-games-with-astra) (2026-09-04) | Body reviewed. Deliver a usable interaction and verify changes under controlled conditions; distinguish simulated measurements from hardware performance. Game tools, publication steps and the author's approval sequence are not universal requirements. |
| [Testing Agent Skills Systematically with Evals](https://developers.openai.com/blog/eval-skills) | Behavior, explicit and implicit routing, false positives and relevant native checks inform skill acceptance. Optional model graders, example test counts, older paths and example build commands do not become mandatory policy or a new evaluation framework. |
| [Shell + Skills + Compaction](https://developers.openai.com/blog/skills-shell-tips) | Routing, on-demand examples, retained working state and untrusted tool-output boundaries apply. Hosted-shell network policies, domain secrets and artifact paths are API-specific examples, not Windows CLI settings. |
| [Prompting](https://learn.chatgpt.com/docs/prompting) and [long-running work](https://learn.chatgpt.com/docs/long-running-work) | Clear outcomes, boundaries and continuation apply to standing instructions. Enter-to-steer versus Tab-to-queue supplies an interactive CLI acceptance route; this is not evidence that a steering scenario has already passed. |
| [Best practices](https://learn.chatgpt.com/guides/best-practices), [customization](https://learn.chatgpt.com/docs/customization/overview), and [subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents) | Task-appropriate context and checks, one owning customization mechanism, and purposeful delegation apply. Preserve explicit provider routing, effort and live-conversation visibility constraints; generic model examples do not replace those choices. |
| [Run long horizon tasks with Codex](https://developers.openai.com/blog/run-long-horizon-tasks-with-codex) | Older GPT-5.3 experiment, not Astra performance evidence. Durable outcomes and observable repair are applicable and already owned by OpenSpec and project memory; the experiment's four-file stack, milestone gates and long runtime are not new pack requirements. |
| [Iterating development workflows with Codex](https://developers.openai.com/cookbook/examples/codex/iterating-development-workflows-with-codex) | Reviewed workflow and postmortem example. The article explicitly identifies its extra files as optional conventions. Reuse existing owners, relevant context and observed verification; do not import its phase-file hierarchy, universal postmortem or separate approval gates into the standing workflow. |
| [Using Goals in Codex](https://developers.openai.com/cookbook/examples/codex/using_goals_in_codex) | Reviewed persistent objectives, evidence-based completion and user-controlled lifecycle. Existing task/spec owners already provide the completion contract; ordinary edits do not need a Goal or another progress ledger. Follow the actual exposed lifecycle tool contract. |
| [Automating repetitive work](https://developers.openai.com/blog/automating-repetitive-work-at-openai-with-codex) | Reviewed practical reuse of relevant context and decisions within existing authority. Runme, WebMCP, notebooks and repeated plan approvals belong to the author's operating scenario, not this pack's required dependencies. |
| [Prompt engineering](https://developers.openai.com/api/docs/guides/prompt-engineering) | Reviewed roles, relevant context, versioned prompts and representative checks. Its generic coding section mixes older examples, Python checks, a TODO tool and reflection after each call with current Astra references. Use the linked, more specific Astra calibration; do not import an unconditional itinerary, language change or formatting rule that conflicts with controlling instructions. API prompt-object retirement does not affect this pack's file-owned prompts. |
| [Astra launch](https://openai.com/index/gpt-6-astra/) | Reviewed user-intent clarification, steering, context management and deployed safeguards. Routine assumptions and independent progress fit the owned principles; consequential decisions still need input. Published benchmark/marketing results are not this pack's acceptance or efficiency evidence. |
| [Safety overview](https://openai.com/index/safety-overview-gpt-6-astra/) (2026-09-03) and [system card](https://deploymentsafety.openai.com/gpt-6-astra) (alignment clarification 2026-09-09) | Overview and applicable prompt-injection, restriction, deceptive-reporting and workplace sections reviewed. Retain authorization, untrusted-data and honest-evidence boundaries. Zero observed failures in an evaluation do not establish universal obedience. Model training, health/cyber benchmarks and monitoring research are not new installer or workflow requirements. |
| [Playco](https://openai.com/index/playco-game-prototyping-with-astra/) (2026-09-03) and [Cognition](https://openai.com/index/cognition-devin-testing-with-astra/) (2026-09-11) | Bodies reviewed: actual game/software use and scoped evidence support the existing outcome policy. Their engines, simulators, manual-fix figures and hoped-for review savings do not impose tooling or prove local benefits. |
| [Perplexity](https://openai.com/index/perplexity-improving-accuracy-with-astra/) | Body accessible on retrieval date but displays 2026-09-14, later than this audit. Treat publication timing as uncertain. Its service-double example does not replace required real integration acceptance or establish current local results. |
| [Configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference) | The launch-linked experimental context option is documented as `features.context_management.experimental_mode`. Installed CLI `features list` reports `context_management` under development and false. This is advertised availability plus local disabled state, not verified context/skill refresh; no feature is enabled by this documentation adaptation. |

The browser retrieval tool rejected several Markdown representations, but direct HTTPS retrieval returned their official bodies successfully; these are not unavailable sources. Retrieval dates in this table are 2026-09-13; publication dates are separate and shown only where verified. Discovery covered the [developer root](https://developers.openai.com/llms.txt), [blog](https://developers.openai.com/blog/llms.txt), [API documentation](https://developers.openai.com/api/docs/llms.txt), [cookbook](https://developers.openai.com/cookbook/llms.txt) and [Learn](https://learn.chatgpt.com/docs/llms.txt) indexes, targeted Astra searches on official sites, and relevant links from those sources. The table resolves the discovered branches relevant to instruction design and this pack's operating guidance; it is not a claim to enumerate every page on the internet.

Index entries for older model-specific prompting, voice, Apps/UI frameworks,
specialist science/business use cases, SDK memory implementations and API
deployment/evaluation infrastructure do not add applicable Astra CLI instruction
requirements. Their distinct products, example stacks and service contracts stay
outside this adaptation. Community posts on an OpenAI-hosted forum are not
official OpenAI guidance. The source conflicts and future-dated case above remain
explicit limitations, resolved through applicability rather than claimed uniform
agreement. No discovered applicable source is being treated as read based only
on a search snippet.

Keep a concise source-to-surface matrix in this change's design while the audit is unfinished. Move only continuing operating guidance to existing owning docs at completion. Do not copy upstream manuals or add an always-loaded research corpus. This provides accountable coverage with less duplication than a separate documentation system.

### 2. Audit broadly, edit by ownership and evidence

Inventory tracked documentation/instructions plus relevant existing untracked working inputs, including hidden directories and strings embedded in source. Cover every discovered file with a disposition. Record source identity and dirty inputs locally so comparison does not attribute existing edits to this change. Detailed private diffs and runtime inputs stay outside public source.

Use `AGENTS.md` and principles as short routing/authority owners, pack skill descriptions as precise selection metadata, and references as optional detail. Read the skill-creator skill before implementing skill edits. Preserve mandatory relevant skill use rather than weakening it to reduce context. Preserve concrete verified tool constraints; a known provider-specific workaround is not obsolete merely because Astra needs less prompting.

A blanket rewrite would risk erasing verified constraints. Appending another Astra policy everywhere would multiply contradictions. Targeted consolidation with one owner per rule is the chosen approach.

| Recommendation | Owning surfaces and disposition |
| --- | --- |
| Clear outcomes, persistence, steering, consequential clarification and readable communication | `global/principles-of-work.md` and project `AGENTS.md` adapted. The per-principle record accounts for every original bullet; required user constraints remain. |
| Narrow skill selection and progressive disclosure | Six owned `SKILL.md` descriptions adapted; detailed verification/RTK/session sections consolidated into directly linked references without changing their contracts. Existing reference examples remain illustrative, not extra mandatory steps. |
| Purposeful delegation and tool use | `global/harness.config.toml` and the Grok role's binding/tool instructions retained. Its description now identifies lifecycle compatibility rather than recommending a universal named level. The existing subscription source validator consumes the role through pinned Bun: all 13 `tests/subscription-config.Tests.ps1` scenarios pass. A TOML comparison confirms description is the sole changed value. This validates metadata/schema compatibility, not a new Grok model run. Explicit provider/effort, continuation-handle/numeric-argument constraints and simultaneous visibility remain binding. No routing or billing changes. |
| Honest, scoped evidence | Native/Python structured-inspection suffixes, disposable outcome prompts, diagnostic data labels, CodeGraph catalogue instructions and continuation handoff text retained. Their schemas, private inputs, read/write boundaries, freshness limits and task-specific fixtures remain required. No executable instruction string was changed, so no changed-string runtime check is claimed. |
| Task-scoped operating context | README/docs routes adapted; the global-connection guide consolidates repeated policy and adoption narratives while preserving results, operating conditions and unresolved recovery. Technical settings and test contracts remain in their runtime owners. |
| Instruction hierarchy and explicit ownership | OpenSpec explore/update confirmation rules are protected external guidance. They may still require a pause when applicable authorization is absent; neither a wrapper nor a competing copy bypasses them. Existing higher-priority user authorization is not erased by the skill. |
| Existing specifications and historical evidence | Project-owned requirements are reconciled by scope; archives retain historical evidence rather than being rewritten as current validation. File-by-file coverage remains a separate completion condition. |

The broad embedded-text scan includes production Rust, transitional Python and
PowerShell, tests and fixtures. It separates prompt constructors from data fields,
identifier occurrences and programmer comments. Retained fixture instructions
such as exact call counts and deliberate missing prerequisites define the test;
removing them under a general concision recommendation would change acceptance.

The delegation baseline now explicitly routes readers to the accepted active
orchestration delta and its pending acceptance. Its historical named-level
requirements are not silently promoted to current routing instructions. The
token workflow guide likewise routes selection and recovery to the current
delegation owner instead of repeating the old Grok-middle/Astra-backup rule.
These documentation corrections do not synchronize or close the separate
orchestration change.

The remaining operating-guide audit separates earlier file-profile discovery
checks from the current shared-default bridge, preserving local-preference
precedence and fallback limits. Subscription guidance uses direct model/effort
selection; custom role schemas remain an optional compatibility contract.
Three cross-cutting main requirements now reference the delegation owner for
routing rather than repeating old middle/reserve choices. Routing and efficiency
baselines explicitly link their active deltas. The orchestration plan labels
its original policy snapshot as historical; verification experiments keep
comparable explicit bindings under the current selection/visibility contract.
No model route, comparison oracle or unrelated task checkbox changes here.

All 38 archived documentation candidates were checked by inventory, requirement
headings, targeted policy/reference inspection and source/link hygiene. Their
starting-state hashes are unchanged. They retain dated decisions and evidence;
this audit does not rerun their behavioral checks or certify their results on
the current working tree. Active artifacts retain their required consumer,
quantitative, runtime and recovery acceptance. Consumer-specific planning prose
and one command example now use generic roles; withdrawal of further work on
the original consumer remains binding. Exact historical inputs stay in private
evidence. Transitional outcome-runner input keys and consumer-resolution code
were assessed as executable data contracts, not prompting rules: changing them
would require migration and compatibility checks owned by the Rust migration.
This instruction adaptation does not claim to repair those runtime portability
limitations or to constitute a comprehensive privacy audit of executable data.

Intersecting work retains its original acceptance:

| Active owner | Boundary preserved by this adaptation |
| --- | --- |
| [autonomous-skill-evolution](../2026-09-19-autonomous-skill-evolution/proposal.md) | Same-session skill recovery, compact/resume, children and global delivery remain required; native memories remain excluded. Advertised experimental context management is not evidence of refreshed skill discovery or a reason to restore ordinary hooks. |
| [improve-installed-tool-workflows](../2026-09-14-improve-installed-tool-workflows/proposal.md) | The shared skills keep current MCP/root/coverage and memory-tool constraints. Global real-consumer and benefit acceptance stays with that change. Description/refactoring work here does not close its tasks. |
| [accelerate-verified-delivery](../2026-09-18-accelerate-verified-delivery/proposal.md) | Real-consumer verification and remaining benefit comparisons are preserved. Our owned loading fixtures and source checks are not substitutes. |
| [migrate-harness-to-rust](../2026-09-17-migrate-harness-to-rust/proposal.md) | Transitional entry points, Rust cutover and their acceptance remain separate. A working AGENTS link or direct native check does not prove that the installed shared-default bridge is healthy. |
| [orchestrate-subscription-agents](../2026-09-20-orchestrate-subscription-agents/proposal.md) | Direct route selection, visibility, resource ownership and recovery remain required. Existing legacy probe strings are retained for their bounded contracts, but cannot be run as visible-conversation acceptance without the required views. |

### 3. Preserve OpenSpec without indirect overrides

Protect `.agents/skills/openspec-*/`, OpenSpec configuration, schemas, templates, generated workflow instructions and installed equivalents. Inventory their actual locations before implementation; protection follows ownership, not only a filename pattern. No wrapper or alternate instruction copy may neutralize their rules.

The inventory also identifies `.agents/skills/.openspec-target`, the six installed
skill links, the installed `@fission-ai/openspec` 1.12.0 package's `bin`, `dist`
and `schemas` trees, and its separate user configuration. Package/configuration
hashes were captured during ownership discovery, not retroactively represented
as starting-state evidence. No task write targets those external locations.
The current 584-file working set has 315 resolved candidate coverage rows,
including configuration/fixture data and source references. A candidate is not
automatically an instruction needing a rewrite. The private per-file matrix
records 39 adapted, three consolidated, 265 retained and eight protected
dispositions, with source mappings and reasons. It includes the three new
on-demand references; raw source bodies and private acceptance state are not
copied into the public pack.

Project-owned specs and active change plans remain editable for consistency. Archives are audited for misleading operational references and publication hygiene, but historical outcomes retain their evidence status. External conflicts receive a protected disposition and a concrete limitation. This makes the user's exclusion explicit instead of promising conformity where edits are prohibited.

### 4. Deliver one real path, then complete the matrix

First adapt a pack-owned principle/skill path for a small documentation correction, verify effective instructions in a fresh outside-checkout consumer and observe its completion. Then apply the same ownership and source mapping to every remaining surface. The first path is progress; every outstanding coverage row and mandatory scenario remains open.

Coordinate with `improve-installed-tool-workflows`, `autonomous-skill-evolution`, `accelerate-verified-delivery`, and affected delegation plans. Preserve their unfinished acceptance and avoid duplicated implementations. If an existing binding requirement must change, obtain the user-owned decision and update its complete owning requirement; do not silently relax it through this new delta.

### 5. Separate document checks, activation and behavior

Use existing source hygiene/local-link checks and strict OpenSpec validation. Compare protected surfaces to the starting state. Verify global loading from two fresh working directories, one containing project instructions, through the installed native prompt inspection route after checking its current help and effects. Retain effective model/instruction evidence.

Run bounded Astra scenarios on disposable owned inputs using the existing project verification/model-probe route: relevant skill selection, small correction with applicable checks, continued work after steering, and justified clarification with independent progress. Read the existing probe's effects and opt-in requirements before execution; planning does not authorize launching probes now. Reuse established tests instead of building a new runner. Missing real steering support is an explicit coverage gap, not a static substitute pass. Failures require affected corrections and reruns, not an unconditional full suite.

## Risks / Trade-offs

- Live global source affects new consumers during edits -> validate each coherent increment and preserve a scoped rollback; restart sessions to verify new initial context.
- Official recommendations are conditional and model-dependent -> record applicability; preserve non-Astra routing and supported compatibility constraints.
- Protected OpenSpec guidance may still cause pauses -> disclose exact remaining constraints; do not report universal conformity across those files.
- Static cleanliness may hide model behavior regressions -> require actual scenario observations and keep unexecuted acceptance open.
- Current uncommitted work overlaps the audit -> use the starting working state, narrow patches and owning active plans rather than resetting or treating HEAD as the sole baseline.

## Migration Plan

Record the starting state and protected ownership, finish official-source coverage, deliver the first verified path, then complete remaining dispositions and edits. Validate links, source consistency, protected boundaries, global loading and behavioral scenarios before closing tasks. Update existing operating guides with current sources and limitations. Restore only this change's edits on rollback, preserving earlier work and unrelated local configuration; verify restored loading in fresh sessions. Publication and Git history edits remain separate actions.

## Current acceptance evidence

The private starting-state comparison accounts for 72 original principle bullets:
60 remain verbatim and 12 have reviewed adaptations in
[the owning principle record](../../../../docs/principles-port.md). The three new
skill references preserve their prior complete sections verbatim. All six owned
skill metadata checks pass. The existing native source checker reports zero
findings over 584 working-tree files. Strict validation covers the 19 main
specifications and affected active changes. Ownership discovery identified eight
protected repository files, six installed OpenSpec skills, 386 installed package
files and one external user configuration; comparison found no changes to those
protected surfaces. Package and external configuration comparisons use their
discovery snapshots as described above, not an invented earlier baseline.

The first owned outside-repository Astra/high documentation run completed its
typo correction with exit 0, and native prompt inspection included the global
source and local project instructions. Its terminal visibility and console
encoding did not establish simultaneous readable conversation views, so that
run did not close task 2.2. The shared desktop required explicit resource
allocation before the repeat; independent work's controls remained outside
this task's ownership.

The user subsequently allocated the right half of the desktop for acceptance.
The repeated documentation case ran with the visible parent above and an owned
Astra/high console below; ready, running and completed screenshots show both.
Native rollout evidence contains the full current global principles and local
project instructions, and identifies `gpt-6-astra` with `high` effort. The model
read the README and applicable verification skill, changed only the typo and
checked the final text and diff; it requested no further approval or application
suite. The exact expected file and native exit 0 were independently checked.
Task 2.2 is satisfied by this repeat, not by the earlier visibility failure.
The launcher reported unavailable shared defaults and used its documented local
fallback: this proves AGENTS/skill behavior, not shared developer-prompt loading
or restoration of the shared-default bridge.

The remaining Rust case used an owned outside-checkout crate in a separate
visible Astra/high conversation on the allocated half of the desktop. Native
input contains the canonical global and local instructions; the model read the
installed memory and verification skills. It asked for the user-owned output
format and implemented parsing and validation while that answer was pending.
A correction submitted during the task was delivered at the next native turn
boundary: case-insensitive counting and lowercase output were implemented, the
empty-input question was answered, and the original requirements remained in
force. After the TSV answer, the model completed code, README, five unit and six
CLI tests, and formatting checks. Eight independent actual-binary cases passed,
including exact sorted output, empty input, length limits and invalid labels
with original line numbers, nonzero exit and empty stdout. The global config
remained unchanged; both owned acceptance consoles exited and closed, and the
parent window was restored. This satisfies task 4.3 for these exercised cases.
An initial desktop text-delivery error was corrected using the inspected native
queue interface to the same visible thread; it is not counted as a model pass.
These observations do not establish universal obedience or measured efficiency.

Model-free installed-launcher inspection from two outside-checkout directories
includes the full current global source once; the repository case places its
local instructions after it. The real local config hash is unchanged. Native
executable identity and explicit Astra/high inspection arguments are retained;
these are configuration/loading evidence, not additional model turns. The existing
`tests/consumer.Tests.ps1` passed 19 native assertions with model requests disabled,
including linked profile/skill/instruction updates and unrelated-state preservation.
A separate owned native-home check exercised connect, edit, restore and link-only
disconnect: each fresh prompt contained only the expected source revision, removal
stopped loading it, and the source and unrelated preferences survived. Real user
links and configuration were not changed for rollback testing. Disposable consumer
registrations were removed through the existing guarded cleanup.
