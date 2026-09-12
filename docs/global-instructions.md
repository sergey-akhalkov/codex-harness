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

The adaptive-workflow rules use the same link: a new session receives the
current decomposition and feedback policy, while the installed
`project-verification` and `token-efficient-workflow` skills provide their
task-specific mechanics. Verify instruction loading separately from successful
task behavior; loading alone does not establish an efficiency gain. The
[delegation guide](agent-delegation.md#how-selection-works) owns workstream
handoff and shared-runtime ownership.

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

## Language defaults and early delivery

Rust is the default programming language and PowerShell is the default shell in
every project, including delegated work. See
[project decisions](project-decisions.md#language-and-shell-defaults).
Model-free `codex debug prompt-input` confirmed that the current source loads
exactly once outside this checkout, with project `AGENTS.md` appearing after
the global text when present.

The early end-to-end delivery philosophy is in the same canonical source and
[project decisions](project-decisions.md#outcome-quality-and-speed). A fresh
child `middle` created from a new parent session can state those rules from
already loaded instructions without tools. That check confirms instruction
loading, not later product builds or unfinished Rust work.

## Engineering judgment and simplicity

The [engineering-judgment decision](project-decisions.md#engineering-judgment-and-simplicity)
uses the same canonical source and live link. Start a new parent session before
checking the updated behavior in another project or in delegated work. Keep that
project's local constraints and the [conversation visibility policy](agent-delegation.md)
in effect; a correct link needs no reconnect or copied instruction body.

Check loading separately from the [accepted task scenarios](../openspec/changes/archive/2026-09-12-prefer-simple-effective-solutions/design.md).
Actual native verification in an existing external consumer and bounded decision
cases establish their exercised behavior; a prompt dump, an agent's recitation or
a generated success report cannot substitute for those results. These checks
are adoption work, not a recurring benchmark or a promise of universal savings.

Adoption on 2026-09-12 used Codex CLI 0.154.0. Model-free prompt inspection
found the complete canonical text exactly once, followed by local instructions,
outside this checkout. Native records confirmed the same text and local constraints
in a fresh Astra/high parent and its designated read-only Astra/high child. Both
conversations had separate visible consoles showing their assignment and live work.

The parent corrected a small document directly, checked its resulting text and
ran the existing native Cargo checks in an owned checkout of a pre-existing
external codec package. All 103 integration tests passed, including complete
and incomplete inputs, payload boundaries and typed rejection assertions. The
checkout, source assertions, manifests and lockfile remained unchanged; no proof
store was added. These results cover the exercised codec behavior, not hardware,
transport, performance, exhaustive inputs or deployment readiness.

The synthetic review preserved durable acknowledgement and concurrent retry
requirements, necessary recovery data and independently usable offline exports.
It recommended one authoritative live value over additional reconciliation and
classified an unobserved test-pass claim as unverified. The child initially linked
the hypothetical report to the unrelated spelling file; the parent rejected that
mapping and checked the real file. A targeted continuation of the same child
withdrew the mistake without further tools or edits. Native runs ended normally;
the accepted parent result and corrected child result passed their output schemas
with no unfinished acceptance items. Prospective deployment questions remain
explicit limits of the synthetic recommendations.

The external run used cached dependencies, Rust/Cargo 1.96.0, offline locked
Cargo, one build job and an owned target directory. A first candidate's offline
preparation was unavailable and supplies no behavioral result. The tested checkout
needed a command-local Git ownership exception; shared Git settings were preserved.
The transitional configuration bridge required an update during concurrent harness
work, so these native runs explicitly received the unchanged live model policy;
the global AGENTS link itself loaded normally. This checks instruction delivery
and bounded behavior, not completion of that separate launcher migration. Detailed
inputs and execution output remain local. No comparative speed or quota benefit
was measured.

The clean tested checkout and completed console windows were removed through
their normal guarded cleanup. Automatic review rejected a combined cleanup that
included forced removal of another owned checkout's generated files; that unused
checkout and local acceptance inputs were retained without claiming cleanup there.

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

Adaptive-workflow acceptance also exercised fresh native tasks: a saved driver
route was corrected in an owned session and consumed by the parent; a reduction
that lost a line-ending failure was rejected; a small edit stayed direct; and
the required integration check still ran. An interrupted handoff retained its
route knowledge, kept old reports distinct from current acceptance, allowed an
independent edit, and completed a fresh parent run and cleanup. The bounded
research answer was consumed without requiring an unrelated implementation.
These synthetic checks establish the exercised decisions, not compatibility
with an installed interactive application.

A real external verification task used identical source and cached artifacts,
the same prompt and model/effort, isolated instruction homes, and four unchanged
contract tests. Source and the pinned product binary remained unchanged; the
parent checked the results and absence of owned fixture processes. The final
skill refreshes the test driver and its required dependencies together, without
rebuilding a separately pinned product or treating an understood preparation
mismatch as a new process regression.

| Observed native task | Elapsed seconds | First passing target set, seconds | Builds / target-set runs |
| --- | ---: | ---: | ---: |
| Preserved baseline | 255.8 | 196.4 | 2 / 2 |
| Initial candidate, no demonstrated benefit | 269.7 | 203.8 | 2 / 2 |
| Corrected candidate | 175.7 | 126.7 | 1 / 1 |

Native elapsed time includes reasoning, tools, preparation, tests, evidence and
restoration. The corrected task took 246.8 seconds through independent parent
acceptance, including 10.0 seconds of external setup and 61.1 seconds of handoff
and parent verification. The entire matched comparison occupied 19.8 minutes,
including shared preparation, the ineffective candidate, correction and repeat;
those adoption costs are not a saving. Detailed inputs, timings and failed
attempts remain in private local evidence. This single comparison demonstrates
the removed preparation/retest cycle under its recorded conditions; it does not
establish universal speed, long-term payback or subscription savings. It adds
no requirement to benchmark ordinary tasks.
