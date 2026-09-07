# Recheck of docs/opencode-kit-audit.md recommendations 1-9

2026-09-07. Working tree `../opencode-kit` HEAD `0e75b94e28f8d860742ef4724f77297e6341630a` (2026-09-03; porcelain count 186). Tests, model probes, lifecycle commands, and official Codex docs were not rerun.

## Findings (audit claims vs source)

1. Rec 2 evidence-class wording is wrong. Source is `"config-declared" | "conventional" | "runtime-observed" | "unknown"` in [opencode-runtime-sources.ts](../../../opencode-kit/tools/opencode-runtime-sources.ts) 70, 458-472. `conventional` is files at conventional locations, not "assumed". Config-declared globs are not expanded because resolved config may contain secrets (725).

2. Rec 2 "version and capabilities of the running server" / "file updated / process still old" lacks Codex implementation evidence. Source hash/restart is OpenCode managed-prompt drift on `agent.compaction.prompt` (`ManagedPromptDrift` 24-28, sha256 548, compare 587, restartBoundary 558-592), not a live Codex session. Current Check probes installer links and a launched permission string ([kit.psm1](../../tools/kit.psm1) `Test-HarnessRuntime` 239, `Test-HarnessConnections` 274, Check 339). Treat running-session staleness as unknown; first Check slice is native config/read origins+layers, `skills/list` duplicates, recorded link-target drift, no model calls.

3. Rec 1 replay-field list is incomplete. [evaluate.ts](../../../opencode-kit/tools/proofs/consumer-outcome/evaluate.ts) 57 requires `cleanup`, `forbiddenEffects`, `friction`, `permissions`, `proof`, `validation`; 73-78 and 188-196 also gate privacy, scenario digest, and environment identity. Saved-sample replay is provider-free; live capture in [consumer-outcome-regression.ts](../../../opencode-kit/tools/proofs/consumer-outcome-regression.ts) 125-180 is OpenCode-specific.

## Confirmed safeguards (already in the audit, not new defects)

- Rec 1 "first" ranking is expected-effort ordering, not a source bug. The audit already rejects wholesale live capture/proof-server.
- Rec 3: statuses live in project docs; do not run `init-project` or add a JSON adapter until an automatic consumer exists ([adapters.md](../../../opencode-kit/docs/adapters.md); [templates/project/AGENTS.md](../../../opencode-kit/templates/project/AGENTS.md) 14-21).
- Rec 4: seed is one replacement `in order to` -> `to` plus `duplicateExceptions` ([instruction-context-quality.json](../../../opencode-kit/config/instruction-context-quality.json) 3-12); default `--check` ([instruction-context-quality.ts](../../../opencode-kit/tools/instruction-context-quality.ts) 22, 164, 198-224). Tuning skill is OpenCode-loader specific; the audit already says start in check mode and not to copy TypeScript plugins.
- Rec 5 `--reuse` is opt-in linking of gitignored dirs ([validate-staged.ts](../../../opencode-kit/global/bin/validate-staged.ts) 12, 29, 75-76, 177-185, 279); the audit already says this is not isolation.
- Rec 7 [openspec-consistency-review/SKILL.md](../../../opencode-kit/global/skills/openspec-consistency-review/SKILL.md) 21 uses "next-increment scope"; the audit already requires rewriting that against complete accepted spec. RCA contract matches ([root-cause-analysis/SKILL.md](../../../opencode-kit/global/skills/root-cause-analysis/SKILL.md)).
- Rec 8: [kaizen.md](../../../opencode-kit/docs/kaizen.md) 114 still shows v1 inbox; [store.ts](../../../opencode-kit/global/plugin/kaizen/store.ts) 41, 414, 639 selects `sqlite-v2` when `OPENCODE_KAIZEN_DATA_DIR` is set. Complain `capture-unknown` ([complain/SKILL.md](../../../opencode-kit/global/skills/complain/SKILL.md) 17, 55, 112) matches. The audit already says not to copy the operator guide as the runtime contract.
- Rec 9: [session-completion-guard.ts](../../../opencode-kit/global/extensions/session-completion-guard.ts) 1-8, 15-31, 36-46 is an OpenCode v2/PTY plugin wrapper. The audit already flags Stop-hook continuation as a separate effect and tells to keep native resume/Goals.
- Rec 6 numbers match: 180-day eligibility ([recall.ts](../../../opencode-kit/global/plugin/project-memory/recall.ts) 18, 195-201), limit 7 and 8 KiB capsule ([project-memory.md](../../../opencode-kit/docs/project-memory.md) 83-87, 134). 180 days is cutoff, not truth.

## Top bounded improvements

1. Do now: extend Check (rec 2, narrowed) with native config/read origins+layers, `skills/list` duplicates, recorded link-target drift. No model calls; no running-session inspection. Highest ROI: Check already exists; harness gap is origin/override/duplicate diagnosis; source collisions (runtime-sources 243-253, 681-688) map without OpenCode loaders.
2. Later package: rec 3 command status in an existing project doc, or rec 5 read-only snapshot without `--reuse`. Defer rec 1 until Check can name the instruction source; defer memory/Kaizen/guard runtimes.

## Scope / unknowns

Read the audit and the kit/harness files cited above; listed session-delivery-context and consumer-outcome files. Parent change `openspec/changes/archive/2026-09-07-diagnose-effective-harness-sources/` exists; contents not reviewed. Not verified: skill/agent counts and byte sizes; claimed 11/11, 9/9, 5/5 runs; work-campaign internals; session-delivery-context algorithms; whether Check launch is a live server or a one-shot CLI probe.
