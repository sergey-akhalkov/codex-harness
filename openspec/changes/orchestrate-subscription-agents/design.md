## Context

See [proposal](proposal.md) for the outcome. The current [delegation guide](../../../docs/agent-delegation.md) and `global/harness.config.toml` give Grok middle priority, retain Astra reserves and allow parent completion after certain incomplete worker responses. Z.AI already has a subscription route, but no senior worker assignment in that policy. The two-executor limit, concise handoffs, explicit model identities and existing billing boundaries are useful foundations.

Read-only inspection found these concrete extension points:

| Current owner | Relevant behavior |
| --- | --- |
| `crates/harness-core/src/launcher.rs` | `task_arguments`, `profile_arguments` and `additional_roots` preserve native `--remote` dispatch; remote/app-server commands do not receive ordinary session overrides. |
| `crates/codex-harness/src/native_read_rpc.rs` | Bounded, one-shot model-free app-server reads in an owned Windows job; useful framing/error-handling precedent, not a persistent task controller. |
| `crates/codex-harness/tests/codegraph_consumers.rs` | Real stdio initialization, explicit `thread/start` with provider fallback disabled and request-id-based response handling. |
| Existing subscription lifecycle and `global/opencodex/` | Reuse the installed provider routes, role links, ownership records, update and recovery. |
| Existing delegation usage and consumer checks | Reuse provider attribution, private evidence and fresh external-session validation. |

Installed CLI `0.154.0` has passed the owned WebSocket two-client/native-child/reconnect/TUI contract check and the isolated ordinary native-entry check with automatic controller startup, a tool effect, final-result checkpoint and service exit. The shared daemon refuses this elevated environment, so the verified path is the design's owned app-server alternative. The [native contract receipt and commands](../../../docs/rust-native.md#native-task-control-contract) own the tested behavior, original failure boundaries and remaining limitations. Official app-server documentation describes model/effort selection at turn boundaries and account-limit observations, but marks this surface experimental. Global activation and quota handoff remain unfinished; current installation keeps the controller disabled.

## Goals / Non-Goals

**Goals:** Keep recovery independent of a successful lead-model response; retain native Codex tools and interaction; add only task state and control needed for ownership, availability and continuation; make GPT conservation and accepted-result latency observable.

**Non-Goals:** A new agent framework or general-purpose chat application, a hosted scheduler, new subscriptions, paid fallback, arbitrary cross-provider encrypted-state portability, universal hard token caps, or an unconditional guarantee of unlimited 24/7 throughput. The required simultaneous conversation windows/panes are in scope. Proxy memory and crash-restart protections remain owned by their existing capability. All accepted task requirements and mandatory checks remain in scope.

## Decisions

### 1. Separate assignment judgment from deterministic recovery

The active lead chooses the workstreams and acceptance boundaries. A small Rust controller owns their dispatch records, availability and recovery transitions. It does not ask another model to classify routine events, poll for progress or decide whether a known exhausted account is available.

Reuse native Codex session execution through its app-server contract. Prefer a verified connection to the local managed daemon; an owned app-server with the native TUI attached is the alternative within the same design when shared-daemon ownership cannot be established. The normal installed launcher connects the required control path automatically; users must not repeatedly launch a second terminal or re-enter the task after failure. Preserve native arguments, explicit profiles, model constraints and project settings in the actual backend session, especially because `--remote` bypasses ordinary launcher overrides.

Before dependent implementation, exercise an owned two-client session: event delivery, model failure, worker survival, reconnect and final-result visibility in the native TUI. The controller needs an authoritative assignment interface and task-scoped event correlation, not transcript scraping or unrelated thread enumeration. Reuse native assignment events where they provide that contract; a narrowly scoped native dynamic-tool interface can supply missing assignment/return fields. Keep that interface in the existing Rust owner, with no second orchestration framework. If the installed protocol cannot meet these requirements, preserve the failed evidence and resolve that concrete integration gap; policy-only delivery does not complete the change.

Alternatives considered: instructions alone cannot react after GPT is refused; proxy-level model substitution lacks task ownership and can resend incompatible state; a separate general-purpose agent framework duplicates the existing execution, auth and tool lifecycle. The native client approach preserves the current user entry point and has concrete local protocol precedents.

### 2. Roles express capability and preserve explicit identities

Assign `zai/glm-5.3`, `xai/grok-4.6` or `gpt-6-astra` explicitly at launch with an effort supported by that exact model. Senior execution and temporary leadership describe responsibilities, not separate agent files. The user prefers this native parameter selection over fixed presets. Retire redundant supplied `middle_backup`, `senior`, `principal` and subscription presets through their owning lifecycle; document the former model/effort mapping so saved instructions can migrate without silently changing accounts. Keep user-owned custom agents and `.agents/skills`. All OpenAI selections remain Astra.

The current native spawn contract exposes independent `model` and `reasoning_effort` fields. Its advertised effort sets differ by model: a name such as `medium` or `xhigh` is not universally supported. Verify the effective binding and provider behavior, reject unsupported combinations explicitly, and do not manufacture a preset for each combination. Existing bounded Z.AI execution selected a model and effort directly; fresh installed-session acceptance remains required.

Temporary Z.AI leadership is a task leadership transfer to a fresh, verified lead binding, not recursive delegation by a worker. The controller transfers the work ledger and allocates the same task-wide two executor slots. It must not launch a second copy of a Z.AI assignment when its worker becomes or supplies context to the new leader. Workers still cannot create their own trees. Direct explicit model selections retain their native precedence and must not be treated as blanket permission for hidden substitution.

Choose compatible reasoning at supported task/turn boundaries. Test a lower-cost setting for routine lead coordination and reserve demanding settings for uncertainty and consequential decisions; adopt changed defaults only after the unchanged outcome checks pass. Do not restart an active task merely to change reasoning settings.

### 3. Persist the minimum state needed to continue safely

Use existing host-private harness storage and serialization conventions. A task record needs a stable task identity, canonical workspace, accepted objective/constraints/authorization, decisions, leader binding, assignments, resource owners, attempt identities, result/check references, availability observations and next action. Raw model history, screenshots, credentials and private consumer evidence stay outside shared source. Store visible handoffs and references rather than copying complete transcripts.

Persist state before dispatch and before changing ownership. Reconcile native session status and owned process liveness after restart; do not infer that a lost connection stopped a worker. Use one task owner and short local serialization of transitions, retaining the existing single-developer assumption. Multiple project tasks share account availability observations but never workspace authority or partial results. A late result is correlated to its attempt and retained for reconciliation, not blindly applied.

For quota exhaustion after a tool mutation, the successor receives existing files, verified effects and outstanding acceptance. Reuse checks only when their inputs remain current. Do not automatically replay an external operation with an uncertain outcome. Read-only and planning boundaries must survive leadership transfer exactly as implementation permissions do.

### 4. Availability is a small state machine, not a race

| Observation | Controller action |
| --- | --- |
| Healthy worker takes time | Preserve owner; wait on events without model polling. |
| Confirmed quota exhaustion | Exclude affected account/model window, preserve work, dispatch a capable alternative. |
| Temporary throttling | Respect usable retry guidance; pace requests/new work without assuming weekly exhaustion. |
| Auth or exact-model rejection | Retain original cause, exclude affected route until relevant conditions change; no credentials or account changes. |
| Transport/proxy failure | Reconcile in-flight effects and use the existing bounded runtime recovery; do not mark all provider quotas exhausted. |
| Empty/intermediate completion | Inspect visible work and result delivery; continue or reassign with a concrete diagnosis, never automatically make GPT finish it. |

Remember observation time, scope and provider reset/retry data. Do not manufacture reset times or treat missing telemetry as unlimited capacity. When reset data is absent, use bounded backoff and a single eligible actual request, not periodic model probes. Distribute retry eligibility across tasks sharing an account to avoid a reset-time burst. Recovery checks do not grant new authority or remove user stop state.

### 5. Leadership changes at safe boundaries

GPT normally owns decisions and acceptance. If its request is refused for quota, the controller has sufficient saved state to start Z.AI leadership without another GPT call. Z.AI continues the agreed task and collects current worker results. Unresolved questions beyond the available models remain pending while independent work continues. If GPT and Z.AI are both unavailable, Grok can complete capable already assigned work; the controller does not invent a substitute for a hard missing decision.

After verified GPT recovery, return leadership at a boundary with no concurrent lead decision, retaining worker ownership. Only one lead can accept results or issue new assignments for the task. Show the successor in its own identified conversation pane/window and record the transition in the task view; retain the previous conversation for inspection. Do not alias distinct leader conversations behind a single visible chat identity.

An active managed runtime may continue across one client disconnect only while the active conversations remain visible through another attached view. If the required views disappear, suspend new model dispatch, reconcile in-flight work and restore the views before continuation; do not silently leave a model loop running. Explicit Stop, cancellation or Disconnect disables dispatch and preserves partial work. Recovery resumes only tasks recorded as active and reconciles their surviving processes first. Continuous execution requires the local machine, runtime and visible conversation views to remain available; suspension or reboot preserves state rather than guaranteeing execution while offline.

### 5a. Make all active conversations simultaneously visible

The confirmed everyday scenario is one leader and up to two executors working while the user sees each conversation in a separate window or pane at the same time. Automatically establish each view before its first model request. Show task/assignment, role, actual provider/model and effort, streaming messages and tool activity, waiting/error/completed state, and available usage without invented quota figures. Preserve conversation history on handoff and completion. Any auxiliary model request, including automatic title generation, must be visible and attributable or disabled. Deterministic transport, event waiting and rendering need no model calls.

Reuse native terminal/chat views where verified; a list requiring chat switching is insufficient. The original single-TUI contract alone does not satisfy this acceptance. The subsequent owned three-window path demonstrates opening the views and displaying a native tool result; task 1.4 still requires integrated dispatch and recovery before live orchestration. Direct injection into a child is rejected and root history injection does not appear live in an attached TUI, so presentation must not depend on that route. View failure must be explicit, preserve in-flight effects, and prevent hidden new requests. Closing one of several clients must not accidentally stop unrelated visible tasks.

For presentation, reuse native Codex in owned visible Windows consoles. The
[native contract receipt](../../../docs/rust-native.md#native-task-control-contract)
owns current checks and limits. Windows [console creation](https://learn.microsoft.com/en-us/windows/console/creation-of-a-console)
provides a separate interactive window and standard devices; the existing Rust
process owner keeps it in a dedicated Job. Window readiness is checked against
the retained process handle and current HWND, and the initial layout places one
lead beside two executor windows. [Windows Terminal panes](https://learn.microsoft.com/en-us/windows/terminal/command-line-arguments)
are an available alternative; direct console ownership avoids adding Terminal
command routing and shared-window lifecycle to this control path. A named empty
native thread can be created by the controller and attached before model work,
which preserves a distinct thread identity and allows title generation to be
avoided. Runtime integration and view-loss behavior remain acceptance work.

### 6. Conserve GPT without underusing the other subscriptions

Start with two executor slots shared across the task, ordinarily one Z.AI and one Grok when their work is independent. Route substantial text/code to Z.AI and visual or suitable routine work to Grok. Return ordinary corrections to the responsible executor; GPT contributes a bounded difficult decision when needed rather than redoing the workstream.

Use authoritative account-limit observations when exposed by the installed service, retaining observation scope and reset windows. Use existing local usage evidence for attribution and comparisons, not an invented quota remainder. Pace new work using observed depletion and time to reset; do not invent fixed percentages of work per provider or a universal reserve percentage without workload evidence. A healthy worker is not preempted merely because account pacing changes. Unknown remainder still permits suitable work until actual evidence changes availability.

For benefit acceptance, predeclare representative text implementation, visual verification and parallel integration tasks, identical inputs/checks, measured baseline and timing tolerance before model-backed runs. Include the cost of briefing, coordination, waiting, validation and rework. Required evidence is lower attributable GPT use, unchanged correctness and no material delivery-time regression beyond measurement tolerance. Fix a failed comparison rather than reducing accepted behavior; an inconclusive result remains unfinished. Exact weekly savings require comparable account evidence and must not be inferred from token counts.

### 7. Reuse current requirements and research

Merge the changed `Bounded collaboration and verification` requirement with the complete delta from `adapt-workflow-through-decomposition`, including runtime resource ownership, validity and restoration evidence and pending integration. Preserve its added workstream-completion requirement and all global-working-principles requirements. Its promise to preserve existing role selection does not freeze the old assignments against this separately accepted routing change. Neither change closes the other's tasks.

The following current sources support the selected mechanisms; the dependency's installed help and isolated behavior determine actual compatibility:

- [Codex subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents): native agents and scoped context provide the execution basis; verify direct model/effort dispatch and native conversation views instead of introducing a second agent framework.
- [Codex app-server](https://learn.chatgpt.com/docs/app-server): use native session/turn control, local transport and limit notifications; its experimental status requires version checks and actual contract tests.
- [Codex pricing and usage](https://learn.chatgpt.com/docs/pricing): allowance accounting depends on more than prompt length, supporting measured attribution rather than token-to-weekly-quota conversion.
- [Z.AI Coding Plan](https://docs.z.ai/devpack/overview): separate short and weekly windows require scoped availability observations; published plan tables are not a live account reading.
- [Grok usage limits](https://docs.x.ai/grok/faq): usage and reset information concern the shared subscription; they do not by themselves establish a callable monitor for the installed OAuth route.

The implemented transport uses pinned `tungstenite` 0.30.0 with only its handshake feature. Its [upstream manifest](https://github.com/snapview/tungstenite-rs/blob/v0.30.0/Cargo.toml) identifies MIT/Apache-2.0 licensing and Rust 1.85 compatibility. Reuse avoids implementing WebSocket framing or introducing a separate async runtime, TLS stack or platform-specific WinHTTP adapter for this local endpoint. The lockfile records the full dependency set. Inspection on 2026-09-12 covered all 18 added packages, their upstream/license metadata, and the two build scripts (compiler probes and generated build-output files); an OSV batch lookup returned no listed vulnerabilities for those versions. The earlier [handshake denial-of-service advisory](https://rustsec.org/advisories/RUSTSEC-2023-0065.html) is fixed in this version. Private receipts retain the query and metadata; this is scoped dependency evidence, not a security guarantee. Any further dependency still requires the same proportional assessment before execution.

## Risks / Trade-offs

- Experimental native control contract -> bind acceptance to the tested CLI/schema and exact two-client behavior; keep unsupported installations on their previous usable path with an explicit limitation.
- Lead replacement while workers survive -> persisted attempt identities, one leader, task-wide concurrency and liveness reconciliation before reassignment.
- A less capable available model cannot preserve quality -> route by required capabilities, retain dependent work and escalate bounded questions; acceptance is unchanged.
- Excess coordination can consume the saved GPT allowance -> concise handoffs, event-driven waits, correction by the owner and matched end-to-end comparisons.
- Unknown or externally consumed account quota -> report observation limits, share account availability and avoid false task attribution or fixed throughput promises.
- Common proxy failure can affect both external providers -> retain native GPT independence and existing proxy resource/restart controls; task retries do not restart unrelated active services.

## Migration Plan

1. Establish the native control contract in owned isolated state and prove one useful tool-using assignment with a quota handoff. Do not accumulate account dashboards or scheduling infrastructure before this path works.
2. Prove simultaneous native conversation views and direct model/effort dispatch, then add private task recovery through existing Rust lifecycle owners. Keep launcher names and native overrides compatible; retire redundant supplied presets with an explicit migration and preserve unrelated user agents.
3. Replace conflicting routing/takeover guidance in the owning global config, delegation and token-workflow records. Reconcile the active decomposition delta when integrating rather than overwriting it from main specs.
4. Exercise update/check/recover/disconnect in isolated installations, including current-source loading outside this checkout and preservation of unrelated configuration, credentials and active control channels.
5. Validate real subscribed bindings, representative recovery paths, a real external consumer and the declared benefit comparison. Only then claim global completion; unfinished implementation and benefit tasks remain open.
6. Rollback restores owned prior links/configuration and removes new dispatch authority without erasing private checkpoints or stopping unrelated runtimes. Explicitly stopped tasks remain stopped after reconnect.
