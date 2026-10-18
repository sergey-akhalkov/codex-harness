# Design

## Context

See [proposal.md](proposal.md) for motivation and scope. This is a cross-cutting instruction, runtime and installation change, so a design is required.

Observed foundations in the current checkout:

- `global/principles-of-work.md` is linked into global `AGENTS.md`; `global/harness.config.toml` supplies developer instructions containing substantial lead orchestration guidance. The former includes common and role-specific obligations together.
- `global/orchestration.toml` owns model/profile routing. `crates/harness-core/src/orchestration_config.rs` resolves native profile overrides and already disables native agent tools for executor sessions.
- `crates/codex-harness/src/executor_assignment.rs` renders task inputs, ownership, base, acceptance, feedback and a repeated executor work cycle. `executor_control.rs` launches with the shared Codex home and supports per-thread configuration overrides; `executor_cli.rs` owns managed launch and recovery.
- `crates/codex-harness/src/skills_cli.rs` already obtains the native skill catalogue. `skill_catalog_refresh.rs` exercises native skill discovery; `orchestration_isolated_install.rs` and the executor integration tests cover existing delivery and recovery paths.
- The installed CLI inspected during exploration was 0.162.1. Its help describes layered profiles and per-invocation configuration. Official [custom-agent documentation](https://learn.chatgpt.com/docs/agent-configuration/subagents#custom-agents) describes role developer instructions and inherited skill settings; [skill documentation](https://learn.chatgpt.com/docs/build-skills) describes native discovery and per-skill disables. These contracts establish a reusable foundation, not proof that the proposed composition has been consumed by a real executor.

The existing global specification requires the entire principles document in every session; the delegation specification rejects per-model/effort role presets. Their deltas make common-core loading explicit and distinguish two responsibility roles from those retired presets. Other assignment, feedback, acceptance and routing requirements remain authoritative.

## Goals / Non-Goals

**Goals:** one owner for each rule, two reusable responsibility roles, deterministic role selection through existing entry points, observable native skill availability, preserved recovery and independent improvement with outcome evidence.

**Non-Goals:** a new orchestration engine, a new skills store, per-profession agents, provider changes, a replacement Codex system prompt, an automatic tuning loop, or isolation from explicit project/user instructions. Planning does not authorize implementation or model-backed acceptance runs in this turn.

## Decisions

### 1. Separate sources, compose at the existing boundary

Keep `global/principles-of-work.md` as the common core. Add canonical role text at `global/roles/lead.md` and `global/roles/executor.md`, with a small `global/roles.toml` descriptor naming their sources and managed skill memberships. These proposed paths are implementation outputs, not files created by this planning change.

Partition obligations by their actor. Shared integrity, evidence, authorization, project guidance and tool-use rules stay common. Lead orchestration rules move out of the common/developer instruction payload; executor standing guidance moves out of repeated assignment rendering where the role now supplies it. Briefs retain task facts and the small concrete feedback addressing information their existing consumer requires. The existing guidance-load contract counts every surface, including required references.

Use native developer-instruction and skill configuration layers through a single Rust-owned resolver consumed by the existing launcher and executor paths. The common core continues through native `AGENTS.md` discovery. Compose the owned role fragment with retained user configuration using explicit source ownership; never use text replacement in an arbitrary user's instruction body. Keep project guidance and explicit settings subject to native precedence.

A separate role file alone would not affect kit-managed app-server sessions. Adding a role paragraph to the current shared prompt would retain the original load. Replacing built-in model instructions through `model_instructions_file` would expand scope and couple the role to Codex's own prompt; it is unnecessary.

### 2. Keep responsibility and model selection independent

The ordinary main-session path applies the lead role, but the existing team-lead activation rules still govern dispatch. The lead can finish a small task directly. Executor spawn applies the executor role automatically; no recurring manual profile or skill selection is added.

Continue to use `orchestration.toml` and native profile resolution for provider/model/effort. Do not encode a provider or model in a responsibility-role identifier. Preserve native agent-tool disabling and the existing prevention of recursive harness dispatch. The executor's autonomy covers its complete local work cycle; consequential changes outside its task return through the existing lead channel.

### 3. Reuse one skills library with explicit memberships

The descriptor classifies kit-managed skills as shared or role-specific. Shared implementation, language, investigation and verification skills remain available to a lead doing direct work as well as to executors. Lead-only orchestration and broad planning workflows are absent from executor discovery. A genuinely executor-specific workflow can be added without copying a shared skill or creating an empty wrapper.

Resolve memberships to the native per-skill configuration for that session. Reuse native catalogue discovery to verify the resulting selection. Preserve explicit disables and unrelated user/project/plugin sources; do not classify unknown foreign skills as forbidden. Protected external skill bodies and workflow configuration remain untouched. A newly managed kit skill must receive a valid membership instead of silently inheriting an accidental default.

The initial catalogue and demand-loaded bodies are separate measurement surfaces. Hiding a catalogue entry is not an authorization boundary; runtime controls still own prohibited delegation.

### 4. Bind roles without mutating another session

Apply role overrides to the session's existing configuration path, not by toggling shared files before and after each launch. Extend existing run state, inspection and visible identity with the responsibility role, source revision and resolved owned skill membership. Record no secrets or full private prompts in public evidence.

Keep the role across spawn, resume, restart and lead succession. Native resume behavior determines whether instructions are retained or rediscovered; capture the effective revision and report changes without inventing a separate prompt-snapshot service. Already active sessions are not hot-rewritten. Existing fresh-conversation recovery adopts current guidance while retaining the worktree, assignment, checks and feedback identity. Legacy runs remain explicitly distinguishable from runs that loaded the new role.

Missing role inputs fail preflight with a repair action. Existing model, visibility, worktree and recovery checks remain in their present owners. No global configuration swap or second Codex home per role is needed; those alternatives would add races or duplicate credentials, sessions and installation state.

### 5. Extend installation and existing evidence owners

Deliver the descriptor and role sources through the current core/orchestration lifecycle. Extend native checks to resolve sources and memberships, and reuse existing installation ownership and rollback. Default instruction delivery, executor launch and skill catalogue resolution must consume the same role definition; an installed file that no runtime reads is not delivery.

Update the existing global-instructions, agent-delegation, installation and project-decisions documents at implementation time. Keep operational detail in those owners and responsibility text in its canonical role. Existing skill-evolution and feedback records can attribute a change to lead, executor or shared guidance; do not add a second tracker or automatically edit prompts after every failure.

The current global specification says 24 KiB while its native-check requirement says 25,088 bytes. The delta aligns the common-core check with the already stated stricter 24 KiB (24,576 bytes), while the existing combined-guidance reduction contract prevents hiding growth in role files. Passing these checks proves size and structure only; semantic coverage and real consumption remain acceptance work.

### 6. Prove the smallest real path, then complete acceptance

First exercise a fresh installed lead and one executor in an owned consumer repository outside the kit: load the intended contexts, discover a shared skill, finish a bounded change, return an evidenced correction and accept the integrated result. Reuse the current visible-conversation and assignment mechanisms.

Then cover role mismatch/missing input failures, concurrent lead/executor catalogue isolation, project instructions and explicit disables, resume after source change, restart, lead succession, legacy recovery and lifecycle rollback. Use native/focused tests for deterministic configuration and transport properties; fake providers cannot establish model behavior or improvement.

For benefit assessment, compare the original and candidate guidance on equivalent representative bounded implementation, correction and scope-boundary tasks. Hold task inputs, base revision, model/effort, tools and applicable checks constant; run comparisons sequentially against owned inputs. Record acceptance quality, missed regressions, lead intervention, correction cycles, total time to accepted result and instruction bytes/words in existing evidence/feedback owners. Include integration and rework; token counts cannot establish exact subscription savings. Preserve an inconclusive or adverse result instead of claiming an improvement or silently weakening acceptance.

## Risks / Trade-offs

- Rule partition loses a safeguard: map existing obligations to their common or role owner and exercise the important success and failure paths; size checks alone are insufficient.
- Over-narrow executor guidance increases lead work: preserve local investigation and correction autonomy, and measure intervention and rework.
- Native inheritance reintroduces lead text or disabled skills: inspect the actual request/context and native catalogue on the supported CLI, including precedence and resume behavior.
- Another session changes while an executor launches: keep role composition session-local and test simultaneous role use; worktrees do not isolate shared configuration.
- A stale run claims current guidance: distinguish recorded role identity from effective instruction revision and show legacy or unverified consumption honestly.
- Smaller prompts do not improve outcomes: retain the measured result and causal attribution; revise the responsible layer without growing both prompts automatically.

## Migration Plan

1. Establish the native composition contract and capture the current matched baseline before changing delivered guidance. Exercise configuration/catalogue resolution without a model first; prove behavior with the installed path afterward.
2. Partition owned rules, add role sources and memberships, and adapt shared resolution, launcher/executor consumers and source checks together. Keep free-text and structured assignments compatible.
3. Verify isolated install/update/check/disconnect/recovery and native consumption outside this checkout, then deliver through the normal global lifecycle. Do not mutate active conversations or unrelated work.
4. Retain existing unfinished runs and native recovery; identify legacy context and describe the ordinary fresh-run adoption path. Record effective revisions when resuming under new source state.
5. Complete representative comparison and reconcile the existing documentation owners. Roll back an unsuccessful delivery through recorded installation ownership while preserving evidence and unfinished work; do not remove accepted safeguards to obtain a better comparison.
