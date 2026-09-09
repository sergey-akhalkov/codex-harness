# Native-workflow skill lifecycle

2026-09-09. Acceptance for `adopt-project-memory-and-native-workflows` tasks
5.1/5.2 used the existing core installer in an independent owned Git root:
`%LOCALAPPDATA%/codex-harness-evidence/workflow-lifecycle-632917d47954476592e8bbf12e14b8ea/`.
`fixture.json` records absolute source, home/user/workspace paths, the original
upstream CLI command and baseline hashes. The fixture contains local AGENTS,
project memory, a user note, an unrelated skill and user-owned configuration.
All preserved files remain uncommitted fixture inputs.

The same source `install.ps1` ran with explicit `-CoreOnly`, owned `-CodexHome`
and `-UserHome`, `-PathScope Process`, and the original `-CodexCommand` from the
live installation receipt. No global PATH or subscription-service action was
needed. The preceding owned preview created no installation receipt.

| Phase | Observed result | Seconds |
| --- | --- | --- |
| Install | Connected; 17 links, 12 skills, 3 agents | 2.79 |
| Update | Connected; all inspected resources still direct source links | 2.91 |
| Repeated install | Connected; preserved exact user file/config hashes | 2.97 |
| Check | Connected; actual neutral native startup verified | 0.80 |
| Disconnect | Owned skill/profile links removed; memory/config/foreign skill preserved | 0.45 |
| Reconnect | Links restored to current source and resources verified | 2.13 |
| Final Check | Connected | 0.83 |

The complete observed episode took 14.24 seconds. Every phase checks exact hashes
of project AGENTS, memory, the user note, foreign skill and base configuration.
That configuration includes Goals=true, memories=false, hooks=false,
code_mode=true and the user's explicit nested context=false value. Disconnect
removes only the owned linked profile; it preserves that base value. While
using the profile, the explicit CLI false override follows the separately
accepted [ordinary runtime contract](native-context-delivery.md).

`*-resources.json` checks symbolic-link targets and matching source/installed
hashes for all three skill descriptors, memory templates, Windows worktree
reference, structured schema, invocation reference and current helper. The
source skill directories remain authoritative; no deployed body copies were
introduced. The existing Python helper remains transitional under the separate
Rust migration; its native candidate does not replace the installed path here.

Two fresh model-free ordinary wrapper invocations, one with the live global
home and one with the owned home, ran `debug prompt-input` from the independent
Git workspace with the explicit context=false override. Parsed native
`<skills_instructions>` catalogues contain `project-memory`, `isolated-worktree`
and `structured-codex-run`; local project AGENTS is present too. Full inputs and
bounded catalogue excerpts remain in `*-prompt-input.txt` and
`*-catalogue-excerpt.json`. Both catalogues use the real user's globally linked
skills; the owned install's separate user-root links are verified by the lifecycle
checks, not misrepresented as a native user-directory override. Discovery is
model-free; actual application is proved by the linked memory/worktree/structured
consumer records. Post-discovery preservation hashes also pass.

The final real global core Check from outside the checkout reports Connected:
19 links, 12 skills and 3 agents. Its receipt and unchanged base-config proof
are in [context delivery](native-context-delivery.md). The earlier global core
Update connected the current profile through the normal installation lifecycle;
source-linked skill updates are visible without copying their bodies. Shared
proxy service and model routing remain unchanged.

A preceding delegated attempt produced no accepted lifecycle work. Its saved
PowerShell helper violated the Rust-only assignment and failed on reserved
`$home` before installation. The agent was stopped and closed; the rejected helper
was removed. Its hash, cause and non-mutation handoff remain in
`workflow-lifecycle-fdb9ebd84e8b4d27b92d1caf7df1276b/rejected-partial.json` and
visible tool history. Parent completed the checks directly, reusing the useful
fixture plan. This was an output defect, not model/auth/quota unavailability;
no reserve or billing substitution occurred. The delegated preparation/rework
is additional cost, not part of the 14.24-second execution measurement or an
efficiency benefit claim.
