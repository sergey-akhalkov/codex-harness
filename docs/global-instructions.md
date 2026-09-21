# Global working principles

[Documentation map](README.md) · [Canonical text](../global/principles-of-work.md) ·
[Working-principles specification](../openspec/specs/global-working-principles/spec.md)

Principles enter the initial context of new Codex sessions through the host
global `AGENTS.md`. That file is a symbolic link to the repository-owned source.
A separate copied body is not maintained. `CODEX_HOME` stays the native Codex
home; this checkout is not used as the authentication or session store.
Connecting the link must not edit local `config.toml`, authorization or model
settings.

Official discovery: [AGENTS.md](https://learn.chatgpt.com/docs/agent-configuration/agents-md)
and [environment variables](https://learn.chatgpt.com/docs/config-file/environment-variables).

## Update and scope

Edit the canonical file in the repository. A new session reads the current text
through the live link; a separate install is not required after each edit. An
already running session must be started again to guarantee an updated initial
context. Creating a child agent from an already open parent does not reload the
parent's initial instructions.

The feedback policy uses the same link. During sustained work it reconsiders
successive different failures in one expensive stage and acts on the next
distinguishing observation, including while an operation is pending. The
installed `project-verification` skill owns check selection, failure attribution
and observed exploration; `token-efficient-workflow` owns prepared replay,
reset/restart comparisons, proportionate measurement and useful delegation.
Routine in-scope improvements need no separate specification. Full acceptance,
resource ownership and recovery remain required. These are prompt instructions,
not a scheduler enforcing future compliance or a guaranteed speed increase.

Verify instruction loading separately from successful task behavior; loading
alone does not establish an efficiency gain. A currently correct link also
cannot identify what an older session loaded. Use that session's retained
native evidence where available, and otherwise preserve the uncertainty. The
[delegation guide](agent-delegation.md#how-selection-works) owns workstream
handoff and shared-runtime ownership. Orchestrated asynchronous development is
activated through the `team-lead` skill, and a user request to use executors
for the current work activates it for that work; ordinary sessions otherwise
spawn no executors.

A later global `AGENTS.override.md` would take precedence over global
`AGENTS.md`. Project instructions add to the hierarchy and may refine the
common rules. Another account or another `CODEX_HOME` needs its own connection.
The link depends on checkout location; after a move, reconnect it to the new
existing source. A Markdown link inside `AGENTS.md` does not include another
document: this connection uses a filesystem link.

The [kit installer](installation.md) recognizes a correct existing principles
link as already connected and preserves it on disconnect. Other platforms remain
separate work. If links are unavailable, fix the cause; copy fallback is
excluded.

## Policy and verification

The [canonical principles](../global/principles-of-work.md) own language defaults,
completion, engineering judgment and feedback. Their
[adaptation record](principles-port.md) explains retained constraints and Astra
source applicability. Read the relevant skill or operating guide for a task;
this page is the connection and recovery guide, not an additional instruction
bundle to load before every edit.

Verify loading with the installed model-free `codex debug prompt-input` route
from the intended consumer directory. Check the effective canonical text,
applicable local instructions and precedence. Do not infer a model, effort or
behavioral result from this dump: retain native turn evidence and observe the
actual task for those claims. Fresh child checks need a fresh parent and the
[required conversation views](agent-delegation.md).

The [Astra alignment change](../openspec/changes/archive/2026-09-13-align-instructions-with-astra-guidance/tasks.md)
records coverage and acceptance. Fresh Astra/high documentation and Rust cases
verified relevant skill use, completion, steering and independent progress during
clarification. The installed launcher used its local fallback: AGENTS and skill
loading passed, but shared developer-prompt loading was not established. See the
[owning evidence and limits](../openspec/changes/archive/2026-09-13-align-instructions-with-astra-guidance/design.md#current-acceptance-evidence).
After the 2026-09-14 shared-default repair, an ordinary local `debug prompt-input`
from an owned outside consumer loaded live developer instructions, including the
screenshot-visibility policy, without the shared-defaults fallback notice.
Experimental context management is injected as `features.context_management.experimental_mode=true`;
native prompt-input does not print that key as text, so effective activation still
needs a new Astra session rather than the dump alone.
Earlier adoption results below apply to their tested revisions and conditions;
they do not validate every subsequent edit.

## Prior adoption evidence and limits

These are bounded results, not recurring benchmarks, universal instruction
compliance or measured subscription savings. Detailed inputs, failed attempts
and recovery evidence remain in their private owning storage.

| Adoption | Observed result and practical limit |
| --- | --- |
| Execution adaptation, 2026-09-13 | Fresh external prompt inspection and the native CLI 0.154.0 Astra/high session contained the revised global text; both skills were discovered and their final references were read in the evaluation. The owned OpenSpec task repaired two delta blocks and a README label using source evidence before one final whole-project validation. Independent checks preserved both requirements, all five scenarios, three main specs and the protected input; all four validation items passed. Ten synthetic continuation cases preserved live-state, reset/restart, cold-start, observer, recovery, held-artifact and delegation boundaries. They are decision evidence, not executed UI/service behavior. |
| Language defaults and early delivery | Model-free outside-checkout inspection found the global source once, followed by local instructions where present. A fresh parent/child check received the rules. This established loading, not later product builds or unfinished Rust work. |
| [Engineering judgment](../openspec/changes/archive/2026-09-12-prefer-simple-effective-solutions/design.md), 2026-09-12 | Codex CLI 0.154.0 Astra/high parent and child had separate visible consoles. The parent corrected a document and ran all 103 integration tests in an owned checkout of an existing external codec package. Source, manifests and lockfile were unchanged. Synthetic review preserved durable acknowledgement, concurrent retry, recovery and offline exports; a mistaken evidence-to-file mapping was rejected and corrected in the same child. No hardware, transport, exhaustive-input, deployment or performance claim follows. |
| [Actionable feedback](../openspec/changes/archive/2026-09-13-make-feedback-workflow-actionable/design.md), 2026-09-13 | A fresh external CLI 0.154.0 Astra/high task loaded global and local instructions, discovered both revised skills and read the verification skill. It repaired and integrated OpenSpec changes in an owned consumer. Parent comparison preserved 30 requirements, ten delta blocks and 21 unrelated files; all 19 main specs passed strict validation. A line-ending defect was corrected; three startup-generated Serena files were retained privately and removed from the consumer. Synthetic cold-start, action/observer, supervisor and invalidated-state cases remain decision evidence rather than executed application behavior. |
| [Adaptive workflow](../openspec/changes/archive/2026-09-12-adapt-workflow-through-decomposition/design.md) | Native tasks corrected a saved driver route and completed parent integration; rejected a reduction that lost the line-ending trigger; kept a small edit direct; and recovered an interrupted handoff without treating old reports as new acceptance. A bounded research answer was consumed without unrelated implementation. These cases do not establish compatibility with an untested interactive application. |

Execution-adaptation native work took 102.7 seconds and its synthetic continuation
73.2 seconds. Consumer preparation through independent parent checks and console
restoration took 509.5 seconds; source/specification work before consumer setup is
additional. The first skill-validator invocation found an unavailable Python
alias; the existing project virtual environment ran both validators successfully.
No dependency was installed. The launcher used the existing local fallback for
unavailable shared defaults; revised global instructions and skill loading were
verified independently. The owned evaluator exited, the parent window layout was
restored, and the other active task's window was unchanged. No comparative speed
gain, universal future adherence or live HMI result is established.

Engineering-judgment acceptance used cached dependencies, Rust/Cargo 1.96.0,
offline locked Cargo, one build job and an owned target directory. An unavailable
first candidate supplied no behavioral result. A command-local Git ownership
exception preserved shared Git settings. The shared-default bridge was not ready,
so native runs received explicit unchanged model policy; global AGENTS loading
worked independently. Completed tested resources were cleaned normally. Automatic
review rejected a combined forced cleanup of another unused owned checkout;
that checkout and its recovery inputs remained private and unresolved.

Actionable-feedback acceptance recovered an invalid window handle before model
execution. The native task took 339.5 seconds, or 496.4 seconds through parent
acceptance and recovery. Its recorded 9 h 50 min adoption window included source
work, checks, unsuccessful visibility preparation, an overnight pause and a user
layout decision. Earlier exploration was outside that window; no comparative
speed gain was established.

The adaptive-workflow comparison used identical source and cached artifacts,
model/effort and prompt, isolated instruction homes, and four unchanged contract
tests. Product source and the pinned binary were preserved. The corrected path
prepared the test driver and its dependencies together, then completed parent
verification and process cleanup.

| Native task | Elapsed seconds | First passing target set, seconds | Builds / target-set runs |
| --- | ---: | ---: | ---: |
| Preserved baseline | 255.8 | 196.4 | 2 / 2 |
| Initial candidate, no demonstrated benefit | 269.7 | 203.8 | 2 / 2 |
| Corrected candidate | 175.7 | 126.7 | 1 / 1 |

The corrected task took 246.8 seconds through parent acceptance, including
10.0 seconds of external setup and 61.1 seconds of handoff/verification. The
whole comparison took 19.8 minutes including the ineffective candidate and
correction. It demonstrates removal of one preparation/retest cycle under those
conditions, not universal speed, long-term payback or quota savings.

## Reconnect

Create the link only when both global instruction files are absent. Existing
files need a separate composition decision.

```powershell
$principlesSource = Join-Path <checkout> 'global/principles-of-work.md'
$codexConfigRoot = Join-Path $env:USERPROFILE '.codex'
$globalInstructions = Join-Path $codexConfigRoot 'AGENTS.md'
$globalOverride = Join-Path $codexConfigRoot 'AGENTS.override.md'

if (-not (Test-Path -LiteralPath $principlesSource -PathType Leaf)) {
    throw 'Principles source is missing.'
}
foreach ($instructionPath in @($globalInstructions, $globalOverride)) {
    if (Get-Item -LiteralPath $instructionPath -Force -ErrorAction SilentlyContinue) {
        throw "Existing instructions require composition: $instructionPath"
    }
}
New-Item -ItemType SymbolicLink -Path $globalInstructions -Target $principlesSource
```

To roll back, confirm the active object is the expected symbolic link, then
remove only that link:

```powershell
$principlesSource = Join-Path <checkout> 'global/principles-of-work.md'
$globalInstructions = Join-Path $env:USERPROFILE '.codex\AGENTS.md'
$instructionLink = Get-Item -LiteralPath $globalInstructions -Force -ErrorAction Stop
if ($instructionLink.LinkType -ne 'SymbolicLink' -or
    [string]$instructionLink.Target -ne $principlesSource) {
    throw 'The active instructions differ from this activation; inspect before changing.'
}
Remove-Item -LiteralPath $globalInstructions
```

The operation removes the filesystem link; the repository source remains. Later
sessions stop receiving principles through that global file. The pack's own
root `AGENTS.md` still routes to the same source for work on this repository.

Loading checks use `codex debug prompt-input` without a model call. They
confirm initial context, not that a model will follow every rule in later tasks.
