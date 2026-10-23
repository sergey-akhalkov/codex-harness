# Design

## Context

See [proposal.md](proposal.md) for the outcome and scope. The existing executor implementation already owns most non-conversation mechanics:

| Existing owner | Reuse |
| --- | --- |
| `crates/codex-harness/src/executor_cli.rs` | Dispatch preparation, assignment/profile validation, pool acquisition and launch binding; separate these from terminal creation |
| `crates/codex-harness/src/executor_control.rs` | Native app-server requests, thread configuration, identity checks, current result/stop/recovery records |
| `crates/harness-core/src/task_worktree.rs` | One repository-wide pool, leases, committed base, preservation and safe reuse |
| Profile resolution and orchestration configuration | Allowed profiles, complete effective configuration, independent executor and pool limits |
| Existing MCP and installation modules | Reuse compatible protocol, registration and lifecycle pieces after checking their server/client roles; their existence does not establish a ready executor MCP server |

Installed Codex 0.164.0 provides native app-server threads, turns, queues, events and client attachment. Its native /agents interface is distinct from the built-in child-agent surface. The current built-in spawn schema has no worktree, provider or profile argument; hooks cannot make unsupported arguments acquire those semantics. Experimental environment features are not a verified replacement.

### Evidence and its limits

Protocol experiments used isolated configuration and synthetic committed worktrees. They changed no global configuration. Raw evidence and process identities remain local; this table records the reusable conclusions.

| Observation | Design consequence / unverified boundary |
| --- | --- |
| One native server admitted two threads in distinct worktrees with the selected external provider/model/effort; the base configuration remained unchanged | Per-thread configuration is a viable foundation. Admission does not prove actual provider/tool execution, full profile consumption or UI visibility |
| Passing a legacy profile name in thread config was rejected; the app-server command also rejected the runtime-only profile flag | Resolve the native profile through the existing owner and pass supported effective overrides; do not forward a profile name blindly |
| A raw effective-config JSON round trip failed on null values; a normalized overlay admitted the threads | Reuse native/source configuration semantics. Do not turn probe-specific null removal into a general configuration serializer |
| Native queue operations worked and one queue submission started a turn automatically | Queue submission has model effects. Avoid a second turn-start; provider request count and usage were not measured and must not be described as zero |
| Threads with no first-turn rollout were loaded but not durable after server shutdown | A returned thread ID is not proof of recovery. Startup failures must preserve the lease and report unavailable history |
| An ordinary daemon started with a short isolated configuration path; elevated daemon startup explicitly refused privileged sharing | A shared ordinary daemon cannot satisfy privilege inheritance for an administrator CLI |
| An elevated standalone stdio app-server initialized and admitted correctly configured threads | Native elevated runtime creation is possible. Protected attachment of the CLI and MCP, actual elevated tool execution and ordinary-client isolation still require proof |
| The controlled process job and an overlong local socket path caused separate probe failures | Do not misdiagnose these fixture constraints as the user's elevation problem; use supported process ownership and endpoint limits |
| Owned probe processes were reconciled and stopped | This establishes restoration of the probe, not production lifecycle acceptance |

### Task 1.1 installed contract

Observed on 2026-10-23 with installed `codex-cli 0.164.0`:

| Contract | Installed evidence and boundary |
| --- | --- |
| Plugin manifest | `.agents/plugins/executors/.codex-plugin/plugin.json` declares plugin `executors`, version `0.1.0`, and `"mcpServers": "./.mcp.json"`. The package is portable: no absolute path, credential or session identity is tracked. |
| MCP configuration | `.agents/plugins/executors/.mcp.json` declares `executors` as a stdio server with `command: "codex-harness"` and `args: ["mcp", "executors"]`. PATH resolution is part of the installed contract. |
| Marketplace | The repository marketplace names plugin `executors` from `./.agents/plugins/executors`. Codex resolves a local plugin source relative to the marketplace root, not the `.agents/plugins` directory; the observed install output is `executors@coding-agents-harness`, enabled, with the expected stdio transport. |
| Caller identity | Codex injects the calling thread as request `params._meta.threadId`. The server accepts only a bounded registry key from that metadata; tool arguments cannot supply or override a lead, run, thread or endpoint. A request without metadata fails before endpoint lookup or pool effects. |
| Endpoint attachment | The server resolves `$CODEX_HOME/harness/lead-endpoints/<threadId>.json`, requires the recorded `threadId` to equal the metadata identity, verifies the recorded app-server process identity is live, connects to that exact loopback endpoint, and requires `thread/read` to report the same thread as `idle` or `active`. A missing or unusable record is an explicit unsupported error with the `codex-harness lead start` remedy; there is no fallback server. |
| Runtime identity | `thread/list` is discovery only: a shared thread store can list an executor thread as `notLoaded` without that thread being active in the lead's runtime. The accepted proof therefore requires `thread/read` on the resolved endpoint and requires the created executor thread to report `idle`/`active` there. The existing CLI spawn path remains a separate app-server and is not evidence of same-runtime attachment. |
| Same-runtime dispatch | The tool calls `thread/start` on the resolved endpoint with the allocated consumer pool slot as `cwd`, `allowProviderModelFallback: false`, the configured default executor profile, the resolved executor role and profile overrides, and `agents.enabled: false`. It verifies the returned identity, routing, title and loaded `thread/read` record before submitting the brief with `turn/start`. After turn acceptance it clears the startup reservation while keeping the slot occupied, so explicit release does not depend on the MCP helper process lifetime. |
| Source and repository | The installed kit source comes from `executor_input::source(None, $CODEX_HOME)`, which reads `settings.sourceRoot` from the installation record. The target repository comes from the calling thread's `cwd` Git root. The kit supplies `global/orchestration.toml`, the configured default profile and pool size; the consumer supplies the pool, worktree siblings, slot records and receipts. A consumer without `global/orchestration.toml` is valid. |
| Owner | `owner` is optional. When omitted, the tool generates `mcp-<caller suffix>-<nanosecond timestamp>`; an explicit owner keeps the collision refusal. No thread, run or endpoint identifier is accepted as authority. |
| Pool and receipt | Slot allocation, assignment validation, base synchronization and profile checks reuse the existing executor dispatch owner. A successful tool result reports run, source, repository, pool size, executor thread, title, slot, consumer checkout, committed base and effective profile; the consumer pool state retains `mcp-<slot>.json` with the same identity. This receipt is separate from the terminal/relay spawn receipt. |
| Refusal | Missing/forged metadata, a mismatched endpoint record, a dead app-server identity and forged tool arguments are refused before a slot claim, thread creation or turn. The forged-argument check rejects unknown fields such as `leadThreadId` or `run`; the exact request returns `isError` with no new receipt or thread. |
| Install and removal | In an isolated Codex home, `codex plugin add executors@coding-agents-harness --json` installed and enabled the plugin, `codex plugin list --json` reported it enabled, and `codex mcp list --json` reported the expected stdio transport. Removal left an unrelated MCP server and settings intact; reinstall preserved it as well. |

The real MCP call used a fresh Codex thread on the existing harness-hosted app-server because the already-running original lead thread had no thread-scoped plugin configuration. That probe thread was a real native conversation on the same endpoint; Codex injected its `_meta.threadId`, the tool resolved that thread's registered endpoint, and the created executor thread and first turn were observed loaded on that same server. This proves the trusted call channel, endpoint attachment and same-runtime thread creation. It does not prove that plugin configuration can be added to an already-started thread without a fresh session; a fresh lead session with the plugin installed before startup remains the ordinary delivery path.

The consumer proof used a synthetic Git checkout without `global/orchestration.toml`, a private isolated home whose synthetic `harness/installation.json` named the kit source, and the same lead app-server. The call omitted `owner`; the generated run label used the caller suffix and a nanosecond timestamp. The tool resolved the kit source and the consumer repository separately, created `consumer-wt1` next to the consumer, wrote `mcp-1.json` under the isolated home's consumer pool state, and reported pool size `6` from the kit configuration. The executor thread ran in `consumer-wt1`, and its first turn executed `git rev-parse HEAD` with the consumer commit and confirmed `global/orchestration.toml` absent.

The first executor turn ran the real brief in the pooled checkout through the configured default profile (currently DeepSeek) and executed `git rev-parse HEAD`, returning the assigned base. The executor thread record reported the pooled `cwd`, title, `deepseek-flash`, `deepseek` and `max`. The `allowProviderModelFallback: false` request field was sent but was not echoed by the thread record, so it is request verification, not an observed runtime assertion. The executor inherited the app-server's sandbox; this slice does not independently prove per-profile sandbox differentiation. The synthetic consumer executor created an untracked `.serena/` tooling directory; the isolated proof lane was preserved as not clean, with no tracked change. The terminal-side watch/pool session display still reads the legacy spawn receipt, so `mcp-<slot>.json` is authoritative for this MCP run until the compatibility paths are reconciled.

This slice does **not** establish automatic delivery while the lead is active or idle, bidirectional question/reply handling, isolated stop/continuation, elevated attachment, two-profile concurrency or native `/agents` UI interaction. Those remain tasks 1.2-1.5 and must pass before broad migration. These are explicit feasibility conditions, not accepted shortcuts or deferred user choices.

Primary contracts: [Codex app-server](https://learn.chatgpt.com/docs/app-server), [developer commands](https://learn.chatgpt.com/docs/developer-commands), [subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents), [hooks](https://learn.chatgpt.com/docs/hooks), [plugin build contract](https://developers.openai.com/plugins/build/plugins), and [configuration schema](https://developers.openai.com/codex/config-schema.json). Recheck the supported installed version's help and generated schema when implementing; website reachability alone does not establish an API contract.

## Goals / Non-Goals

**Goals:** Keep native Codex responsible for model execution, conversation history, input delivery and presentation. Keep the harness responsible for its existing task/worktree lease, effective profile, caller-to-run relationship and acceptance lifecycle. Make the normal path intent-driven and preserve current behavior outside the changed executor surface.

**Non-Goals:** No Codex source changes, custom agent runtime, second mailbox/transcript store, plugin manager, replacement board, automatic merge, new provider route, or conversion of unrelated harness features. A messaging-only plugin would duplicate identity and lifecycle ownership and is not part of this change. Native child-agent registration is unnecessary.

## Decisions

### 1. One plugin with a thin Rust MCP entry point

Expose operations over shared existing executor services rather than invoking command strings or moving the whole CLI into MCP. Keep executable compatibility adapters using the same domain logic. The installed plugin bundles the MCP connection and concise owned skill guidance; native Codex owns plugin discovery/enabling, while the kit's current lifecycle owns delivered artifacts and recovery.

Start with these consumer operations; final exported names follow the installed plugin namespace without adding aliases:

| Operation | Necessary inputs and result |
| --- | --- |
| `executor_status` | Current caller scope; bounded owned runs, held slots, configured profile choices and availability, with native thread/detail references |
| `executor_spawn` | Assignment, consuming repository when not already bound, committed base when needed, optional allowed profile; returns verified run/thread/slot/base and actual binding |
| `executor_message` | Literal content plus run or request reference when needed; request, notification or reply intent; returns correlated native delivery evidence |
| `executor_wait` | Owned run references and bounded wait; returns actionable question, terminal result, timeout or observation failure |
| `executor_stop` | Verified run reference; returns actual isolated stop outcome and preserved continuation identity |
| `executor_resume` | Saved run reference and explicit exact-continuation or fresh-recovery mode; returns preserved checkout and resulting thread binding |
| `executor_release` | Run/task reference and existing acceptance or explicit-discard evidence; performs the established safe pool release only |

Use native list/read/wait facilities for conversation inspection wherever they cover the need. The status/wait adapters add only existing pool/run and unresolved-request semantics absent from native thread state; they must not become another conversation store or polling scheduler. Do not publish separate tools for every internal helper or old command option.

MCP arguments carry literal structured data. Ordinary operations derive identity and installation state from verified bindings. Invalid mode combinations and ambiguous targets fail before effects; the lead never constructs endpoint URLs, temporary message files or full address tuples.

### 2. Attach to the native runtime that owns the lead

The preferred path connects the plugin to the native runtime already serving the originating CLI. If supported native startup does not expose the necessary connection, the only permitted adaptation is minimal bootstrap in the existing installed launcher: create a session-owned native app-server and attach the one native CLI client to it in the same terminal. Executors become threads in that runtime, with no additional conversation engine or per-executor UI processes.

A caller/thread binding must come from a supported native launch/call channel whose trust is demonstrated. User-supplied thread IDs, process titles, cwd, most-recent session selection and marker-only environments are insufficient. Validate both the originating lead and the executor's restricted return channel. Thread switching in /agents must not change an MCP caller's authority accidentally.

Use the CLI's privilege boundary. A standalone session-owned runtime is the candidate for elevated sessions because native elevated shared-daemon startup refuses by design. Supported authentication and attachment must also isolate integrity levels; same-account authentication alone is not assumed sufficient. Do not install a privileged service, silently lower privileges, expose an elevated endpoint to ordinary clients, or restart a live unrelated server.

**First-slice decision:** prove this attachment and authority path through a real installed plugin and native CLI. If the current extension points cannot provide it, preserve the working partial slice and exact failure and stop dependent migration. Neither separate executor terminals nor a file relay satisfies the agreed outcome. Any change of that outcome needs a new user decision; this plan grants no fallback architecture.

### 3. Profiles are resolved configuration, not model-name substitutions

Reuse the existing profile resolver and allowed executor bindings. Resolve native configuration with its source/precedence semantics, then apply only supported per-thread settings. Preserve provider authentication references, effort, features, instructions, MCP/tool availability and execution policies; do not copy credentials into tool results or repository artifacts. Keep the lead configuration unchanged and disable provider/model fallback.

Prove actual consumption, including the configured external executor profile and its differing feature settings, through a real request and native tool use. Metadata returned by thread/start is necessary but insufficient. A setting that is process-global and cannot differ as required is a preflight limitation, not permission to silently inherit the lead's value. Existing descendant-spawning restrictions must leave the executor's authenticated MCP reply channel usable.

The current local default profile remains a configuration choice, not a plugin constant. Acceptance follows the owner's currently allowed routes; it does not enable unconfigured providers for testing.

### 4. Native input and events own conversation exchange

Persist only the necessary originating-lead/run/thread/lease relationship and correlation references in the existing receipt/task lifecycle. Use native queue/turn/event identities for delivery reconciliation, native history for content and results, and existing board records for durable task decisions. Retire redundant relay/spill/transcript owners only after their accepted behavior is consumed through the new path.

For an active turn, use the supported addressed steering operation with the expected turn identity. For an idle conversation, enqueue supported native input and observe its actual start; queue acceptance can start a turn, so do not blindly also start one. Reconcile active-to-idle races and uncertain acknowledgements without resending text. Identical intentional messages remain separate operations.

The MCP call must not block waiting for its own recipient to answer. Native events drive request/action-required and completion delivery to the lead, including while it is waiting in a tool. A lead need not open /agents or ask for status to receive them. Verify this reentrant path for deadlocks and duplicates. Sending a correction does not kill the recipient's running command.

An unanswered request keeps the run nonterminal without model keepalives. Notifications do not create a reply hold. A reply resolves its exact request and resumes the same conversation; stopping or replacing a run invalidates delivery to that old identity, not to its successor. A temporarily absent lead remains the recorded recipient until supported explicit recovery changes ownership.

### 5. Shared runtime lifetime is separate from executor outcome

A native turn/result determines an executor outcome; app-server exit does not. Keep the existing distinction between completion, output defect, failure, stopped state and lead acceptance. Preserve the bounded result and native detail locator after UI navigation. Multiple user/tool turns in the same thread must not let an earlier completion finish newer work.

Stopping an executor uses exact-thread native interruption plus verified containment of its owned child work. A shared app-server process tree is never the target of run-local stop or cache-loss protection. If native containment cannot establish the promised isolated stop, that fails the first slice rather than weakening urgent-stop acceptance. Preserve current cache thresholds, evidence rules and native warning presentation; only its process-isolation assumption changes.

Exact continuation reconciles ownership and reconnects without resetting the pool. Fresh recovery creates a new bound thread from the preserved assignment and bounded progress, retains the predecessor, and checks interrupted effects. A failed pre-turn thread may have no durable rollout; keep the worktree and expose that fact. Normal release remains an explicit existing acceptance/discard operation, retaining ignored build caches.

### 6. Remove obsolete guidance at the working cutover

The active source owners include:

- `global/roles/lead.md` and affected executor role/context generation.
- `.agents/skills/team-lead/SKILL.md`, `.agents/skills/board-workflow/SKILL.md`, and `.agents/skills/token-efficient-workflow/references/session-reuse.md`.
- `docs/agent-delegation.md`, affected sections of `docs/rust-native.md`, `docs/project-decisions.md` and installation guidance.
- Generated briefs, tool descriptions, ordinary help, error and recovery hints, and lifecycle-delivered copies.

Remove migrated CLI mentions, fallback recipes and old-to-new tables from these required surfaces, rather than keeping both workflows in agent context. Do not author parallel machine-local skills. Protected external OpenSpec workflows and unrelated guidance stay untouched.

Existing command/package compatibility remains in code and targeted regression checks, not as active workflow advice. Retain only compatibility internals with actual callers; do not delete working paths solely because MCP no longer advertises them. Measure the existing byte/word and representative loaded-context acceptance with fixed assignments, including metadata and required references. The planning change does not switch installed instructions to tools that do not yet exist.

## Risks / Trade-offs

- **Native attachment or caller identity is unavailable:** the first real slice fails with a concrete API/identity boundary; retain partial work and stop broad migration.
- **Profile admission differs from actual execution:** verify provider/tool calls and effective settings, including features, before claiming profile support.
- **Elevated shared daemon refuses startup:** use a proven session-owned native attachment or report the unsupported boundary; ordinary daemon success does not prove administrator support.
- **Shared-process interruption affects neighbors:** test generation and a running child command beside a healthy executor; failure blocks replacement of the current control owner.
- **Native queues start turns or wake a busy lead differently than expected:** observe actual events and model-visible receipt; never infer delivery from a successful local write.
- **Plugin reconnection duplicates work or leaves questions unanswered:** reconcile exact native and lease identities, retain indeterminate states and test recovery without replay.
- **Context cutover leaves stale loaded instructions:** verify fresh installed lead and executor sessions as well as generated/source text; saved conversations may require supported fresh-context recovery.
- **Tests rely only on a toy fixture:** use fixtures for deterministic races/failures, then the existing real external-consumer acceptance with locally supplied private inputs. No quota exhaustion or newly enabled billing route is needed.

## Migration Plan

1. Deliver a minimal real MCP/native slice using existing pool and profile code. Prove two conversations in one CLI, distinct configured profiles/worktrees, a question/reply during active and idle states, automatic result return, isolated stop and continuation. Exercise ordinary and elevated attachment with actual command privilege.
2. After that proof, extract only shared executor logic needed by MCP and compatibility callers. Finish the changed contracts and targeted regression scenarios, including cache protection and preserved work.
3. Connect plugin packaging and registration to the existing install/update/check/recovery/disconnect lifecycle. Verify the installed artifact from a fresh external consumer without checkout-local imports.
4. Cut active guidance over to verified MCP names and remove obsolete context. Update the owning decisions with the new visibility and communication contract and retarget links; do not add historical migration manuals.
5. Complete integrated external-task acceptance, then record verified support and remaining version constraints in the existing guides. Only then retire proven-unused presentation/relay pieces.

A failed candidate leaves the currently installed working artifact and all task/worktree records intact. Lifecycle recovery restores the prior owned artifact without resetting work, reparenting active threads or overwriting unrelated settings. Recovery reports the actual available capability; it does not add a silent legacy dispatch recipe to new agent guidance.

