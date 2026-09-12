## Context

See [proposal.md](proposal.md) for the motivation and scope. The existing [global principles](../../../global/principles-of-work.md) already cover simple designs, research, complete outcomes, proportionate checks and adaptive work. The missing emphasis is comparative engineering judgment: an individually useful mechanism can still be an unnecessarily costly way to achieve the goal.

The host global `AGENTS.md` is already linked to the canonical source. [Global instructions](../../../docs/global-instructions.md) owns loading, update and rollback. New sessions receive the source through that link; an existing session is not presumed to reload it. The current [decisions](../../../docs/project-decisions.md) and main specification protect externally maintained OpenSpec workflows and reject a separate skill or risk-scoring system for the general philosophy.

Scoped inspection also found useful existing owners: `project-verification` discovers native checks and preserves their execution identity; `token-efficient-workflow` bounds retrieval and discourages one-off automation layers; `project-memory` owns concise verified lessons; the delegation guide accounts for coordination and rework. Their bounded diagnostics and project records are compatible with this change. There is no demonstrated need for another tool or workflow.

## Goals / Non-Goals

**Goals:** Make the agreed behavior effective at ordinary task selection, consequential planning, verification and revision; keep the always-loaded instructions cohesive; demonstrate a useful path outside this checkout.

**Non-Goals:** Automatically redesign existing products, reduce accepted functional scope, delete historical data in another project, change model routing, or add a general evaluation platform. This design governs instruction adoption; it cannot guarantee perfect judgment or universal speed improvements.

## Decisions

### 1. Revise existing owners instead of multiplying policy copies

Keep the general rule in the existing principles and consolidate nearby wording where it would otherwise repeat the same instruction:

| Existing section | Responsibility |
| --- | --- |
| Autonomy and authority | Independent technical judgment, constructive disagreement and explicit authority boundaries |
| Everyday use and design discovery | Separate desired outcomes and binding constraints from proposed means; carry the distinction into requirements and design |
| Simplicity and reuse | Actively seek a simple complete path and justify complexity comparatively over the whole lifecycle |
| Quality and evidence | Native reproducible verification, observable results and legitimate retained data |
| Speed, feedback, and recovery | Reconsider a burdensome model and retain verified reusable conclusions through existing memory ownership |

Update `docs/project-decisions.md` with the concise confirmed decision and `docs/global-instructions.md` with applicable operation and acceptance facts. Review the existing `project-verification`, `token-efficient-workflow` and `project-memory` skills, their directly implicated references, `docs/token-workflow.md`, `docs/agent-delegation.md` and the repository instruction route for genuine conflicts. An already compatible owner needs no edit. Correct any actual conflict locally and validate its behavior; do not copy the full rule into each skill. External OpenSpec files remain untouched.

Alternative considered: a new simplicity skill, checklist or reviewer. It would add selection and maintenance overhead for behavior that must already apply to ordinary work. The existing global source and task-specific owners cover the current need.

### 2. Make simplicity a choice rule with a stopping condition

For a consequential choice, establish the complete outcome, then consider the simplest plausible path using the existing platform and compare another approach only where it could matter. Look for work that can disappear through a better representation, a single owner, fewer independently mutable values or removal of an unnecessary stage. Evaluate both the produced system and the effort required to build, understand, verify and use it.

A complexity justification names the requirement or observed constraint and explains why the simpler alternative fails it or costs more overall. It belongs in the existing design or a concise explanation where the decision matters; ordinary edits do not acquire a decision document. The rule sets no fixed number of alternatives, duration of reflection, dependency count or line-count target. Stop when the consequential choice has sufficient evidence and the accepted path can be exercised.

An appealing simplification that loses a required property is not an acceptable implementation choice. Explain material trade-offs, preserve explicit constraints and continue independent authorized work while a genuinely user-owned decision is unresolved. Do not repeatedly challenge a settled constraint without new information. Accepted technical choices can be revised on evidence; actual product requirements are not silently rewritten to make a mechanism removable.

Alternative considered: strengthening only the existing instruction to justify every addition. That still permits a plausible benefit to stand in for a comparison. Mandatory multi-option design reports would overcorrect by adding work to routine changes.

### 3. Put confidence in executable checks and grounded observations

Use the consuming project's established test runner and relevant integration, end-to-end or benchmark routes. Preserve meaningful expected results, actual entry points, reproducible inputs and sufficient environment or build identity for the claimed behavior. A failing check must remain a failure, including when a generated report says otherwise. A manually prepared expected fixture can be useful; an agent's assertion that a run succeeded is not evidence that the run occurred.

Special hardware, external systems, audit requirements or diagnostic needs can justify an adapter or retained output. Demonstrate the precise gap and actual consumer, keep collection reproducible where applicable, and use the existing storage and cleanup lifecycle. Preserve product data and unresolved recovery state. A general demand for proof does not itself require a custom publication protocol or a prohibition on files.

Alternative considered: banning JSON, reports or all durable results. This would discard legitimate fixtures and diagnostics and could violate recovery or explicit audit requirements. The choice is based on purpose, verification strength and total cost, not file format.

### 4. Use a bounded adoption check, then learn through ordinary work

First check fresh global loading outside the harness using the existing model-free `codex debug prompt-input` route, including a working directory with its own project instructions. Read installed help before invocation. Verify the source and applicable local constraints, and confirm a useful bounded child task receives the guidance from a freshly started parent. Reuse an existing independent acceptance subtask for that child; do not delegate every case.

Then exercise the following finite acceptance situations with native sessions and inspect the decisions, tool results and resulting work. At least the proof-request case must run an actual native verification path in an owned checkout of a pre-existing external repository with meaningful assertions. Exact private inputs and local paths are supplied at implementation time and remain outside this pack. Other cases may be bounded synthetic decision tasks, whose limits must be stated.

| Situation | Observable acceptance |
| --- | --- |
| A small authorized correction with a clear solution | Completes the correction and applicable check without a new abstraction, checklist, repeated approval or unrelated research |
| A request for rigorous proof in the external consumer | Uses its native verification path; observes a representative successful condition and a meaningful failure; preserves the assertion and reports its scope without introducing a proof store |
| A concrete requirement needs complexity | In a recovery/concurrency scenario, retains the required guarantees and legitimate recovery or diagnostic data; explains why the simpler proposed approach is insufficient |
| A proposed mechanism becomes burdensome | Identifies a simpler model, explains the practical difference and revises the design while preserving the outcome; a binding-constraint variant prompts discussion before any material change |
| A claimed success lacks an observed run | Classifies the result as unverified; a report, status label or hash cannot promote it to a pass |

Inspect the artifact and instruction diffs for added machinery, contradictions, privacy leakage and unsupported claims. Record the concise supported result and any limitation in the existing owning guide; keep detailed execution output in appropriate local storage only when needed. Preserve failed attempts until the cause is understood and report the full scope of observations. Repeat affected cases only after a change, failure or unresolved concern. Reciting the rules and loading them are not behavioral acceptance; missing external-consumer execution keeps its task open.

These checks establish the exercised behavior, not a general efficiency percentage. Any later claim of improved speed or cost requires a relevant comparison including preparation, coordination, verification and rework. A benchmark suite, periodic reviewer and new result format are not adoption prerequisites. During subsequent work, retain a useful verified lesson in its existing owner when one arises; do not manufacture an entry for every task.

### Research and applicability

Primary sources inspected during the exploration on 2026-09-12 support the choices below. The engineering works supply design heuristics, not measurements of this agent's behavior.

| Source | Applied conclusion |
| --- | --- |
| [Rich Hickey: Simple Made Easy](https://www.infoq.com/presentations/Simple-Made-Easy/) | Reduce intertwined responsibilities; familiarity and ease of initial construction do not establish simplicity |
| [John Ousterhout: The Nature of Complexity](https://web.stanford.edu/~ouster/cgi-bin/cs190-winter18/lecture.php?topic=complexity) | Use scattered changes, cognitive load and hidden dependencies as diagnostic signals |
| [Martin Fowler: Yagni](https://martinfowler.com/bliki/Yagni.html) | Avoid speculative mechanisms while maintaining code health and the ability to change |
| [Google: What to look for in a code review](https://google.github.io/eng-practices/review/reviewer/looking-for.html) | Assess unnecessary generality and complexity in both production code and tests |
| [Rust: How to Write Tests](https://doc.rust-lang.org/book/ch11-01-writing-tests.html) | Reuse the standard setup, execution and assertion model before adding verification machinery |
| [OpenAI: Custom instructions with AGENTS.md](https://learn.chatgpt.com/docs/agent-configuration/agents-md) | Deliver persistent defaults through the existing global/project instruction chain and validate a fresh session |

## Risks / Trade-offs

- Simplicity becomes underengineering -> Keep functional, performance, recovery and required-check invariants explicit and exercise the justified-complexity case.
- Independent judgment becomes silent scope change -> Preserve binding constraints and distinguish technical choice from a material change requiring agreement.
- Reflection delays useful work -> Use consequence-driven comparison and a clear stopping condition; exercise the routine case.
- Anti-evidence wording damages reproducibility -> Preserve useful fixtures, diagnostics, audit and recovery inputs and require actual observations behind claims.
- More prose obscures the rule -> Consolidate existing wording and keep task-specific mechanics in their existing owners; do not add a fixed documentation or metric quota.
- Instructions load but behavior remains poor -> Keep loading and task acceptance separate, correct observed failures and scope claims to tested cases.

## Migration Plan

1. Integrate the rule into the existing principles and reconcile affected pack-owned records while preserving unrelated working-tree changes and unfinished accepted work.
2. Run applicable source hygiene, local-link and OpenSpec checks. Validate any task-specific mechanics actually changed through their existing checks.
3. Use the existing live-link installation for fresh external parent and delegated acceptance. Do not infer an update in already running sessions or reconnect a correct link merely to exercise setup.
4. Complete the bounded task cases, record concise results and limitations in the owning guide, and close tasks only on their actual acceptance. Synchronize the delta through the normal OpenSpec completion workflow.

Rollback is a scoped reversal of this change's instruction and documentation edits, preserving unrelated work and any unresolved task evidence. Fresh sessions then load the restored source through the same link. No new service, dependency or data migration needs removal.
