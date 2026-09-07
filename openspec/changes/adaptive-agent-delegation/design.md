## Context

See [proposal.md](proposal.md) for motivation and [the contract](specs/agent-delegation/spec.md) for acceptance. Codex CLI 0.153.4 already reads a linked `harness` profile and personal agent directory. Subscription routing has a separate linked agent directory that disappears on disconnect. Its installed Grok reviewer fixes `xai/grok-4.6` at `high`. Native mixed-provider delegation v1 is verified; OpenCodex 2.44.0 forwards existing ChatGPT authorization and owns separate xAI OAuth. The live account model list returned Grok 4.6 and 4.5 on 2026-09-07. The native catalogue supports Astra high, xhigh and max.

Current official contracts provide additional `developer_instructions`, standalone custom agent TOML files, concurrency limits and agent enablement. They do not establish a configurable hard per-subagent reasoning-token cap. App-server offers read-only account rate-limit observations; native sessions record model, reasoning and token usage. These are different evidence sources and must not be conflated.

## Goals / Non-Goals

**Goals:** reuse native orchestration, keep model bindings outside task skills, activate through existing direct links, prove correct level execution early, and measure complete accepted work with bounded opt-in probes.

**Non-Goals:** a new scheduler/service, account pooling, paid API billing, an automatic model tournament, mandatory planning documents for every delegated task, or a guarantee that every task becomes both cheaper and faster. The acceptance benchmark is bounded evidence, not a substitute for later actual-project outcomes.

## Decisions

1. **Universal native definitions.** Put `middle` in `global/opencodex/agents` with Grok 4.6 xhigh. Put `middle_backup` (Astra high), `senior` (Astra xhigh), and `principal` (Astra max) in `global/agents`. These exact settings and Astra-only OpenAI usage are explicit user decisions. The backup is an alternate implementation of middle, not a fourth intelligence level. Exact bindings are independent of task skills and can be changed within user constraints after validation. All children perform assigned work directly and disable their own spawning through native agent configuration. This prevents unrequested recursive delegation without a new runtime.

2. **One compact primary policy.** Add `developer_instructions` to the shared profile and set the native concurrent child limit to two. Keep the portable philosophy unchanged. The policy explicitly authorizes worthwhile autonomous delegation, prefers named middle when available, selects the backup only on observed unavailability, and permits direct senior execution and rare principal consultation. It covers concise assignments/results, disjoint ownership, progress-aware retries, meaningful checks, and nonduplication. Avoid unconditional global Grok model defaults: they would survive subscription disconnect and misroute generic children. The native named definitions are the exact-model mechanism; orchestration decides explicit switches.

3. **No ceremonial escalation.** A cognitive blocker can go directly to principal when evidence warrants it. Supply a focused question, verified facts, reproduction and failed hypotheses. Preserve code and records before switching. Time/output requests are advisory and clearly labelled; concurrency and disabled child spawning are runtime controls. A principal answer is independently checked by the primary agent, which retains integration ownership. Infrastructure problems do not trigger principal. Routine retries do not require user permission.

4. **Explicit auxiliary costs.** Inspect pinned search/vision configuration and disable implicit OpenAI helpers on the Grok route, using an explicit same-subscription capability where supported or a visible limitation otherwise. Keep native OpenAI capabilities available to explicit GPT assignments. Validate actual effective configuration without interrupting a live dependent session. After the reported memory-limit termination, reconnect the already stopped service with a 2048 MiB job ceiling; administrative commands retain 768 MiB. Raising the ceiling adds headroom and is not proof of a leak fix. Do not infer subscription quota from API price fields.

5. **Measurement with a small standalone helper.** Extend the existing bounded native probe approach, keeping reports in a caller-selected private output directory. Aggregate latest cumulative usage once per correlated thread; track missing records and descendants, provider/model, reasoning and elapsed time. Compare matched direct-Astra and mixed executions on meaningful isolated coding/review fixtures with deterministic acceptance. Include parent coordination and rework. Keep account-level quota snapshots separate because concurrent sessions and rounding confound attribution. The helper is for repeatable verification and opt-in evaluation, not a production orchestration layer.

6. **Preserve lifecycle ownership.** Existing links activate profile/agent source changes for new sessions. Update inventory requirements and source validators where necessary. Replace active references to the retired role, preserve historical records with migration notes, and leave unrelated dirty files intact. Use existing isolated lifecycle tests for disconnect/update behavior; do not stop, restart or disconnect the global service from its own Codex session.

## Risks / Trade-offs

- Token savings can be erased by coordination, large initial context or reviewing twice -> compact policy/briefs and matched end-to-end measurement; correct poor routing based on observed results.
- Tests can claim a model by name while another served it -> assert native model/effort plus correlated proxy route where available; never rely on self-identification.
- A model request can fail after partial edits -> preserve ownership and changes, inspect current state before retrying or reassigning.
- Model profiles override inherited instructions -> keep each universal child's task/verification contract self-contained, preserve global project instructions and available skills, and verify prompt discovery.
- Auth/quota telemetry may be unavailable or rate-limited -> report unknown rather than treating it as zero; availability follows actual failures and recovered service evidence.
- Current session caches old definitions -> verify new consumers outside the checkout; use the already installed Grok role only as a temporary implementation aid before retirement.
- Global config changes affect other sessions -> scoped source edits, native config validation and existing link ownership; no broad reinstall or service interruption.
- Existing Python/LSP diagnostics are unrelated and incomplete -> limit correction to touched behavior, retain honest diagnostic scope and run relevant checks.

## Migration Plan

1. Record the confirmed decisions and complete the OpenSpec artifacts. The user's explicit instruction authorizes applying immediately after planning, overriding the propose skill's suggested extra turn.
2. Add level definitions and the primary policy, validate them with the actual CLI, then retire the named reviewer and migrate its active validators/consumer references.
3. Apply and validate auxiliary-cost configuration through its supported mechanism. The reported memory-limit incident left the service stopped; recovery through the installer loaded the updated source with the documented 2048 MiB service allowance. Verify effective management settings and preserve the recovered process during subsequent acceptance.
4. Run static/configuration and isolated lifecycle checks, actual named-level and policy scenarios from outside the kit, then matched efficiency comparisons and a focused independent review. Resolve material findings and mark only demonstrated tasks complete.
5. Document current activation, evidence and limitations. Roll back only this change's source additions/edits if needed; existing direct links expose the prior source to subsequent sessions. Standard kit/subscription disconnect remains the supported complete removal path.

## Sources

- [Codex subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents), [configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference), and installed CLI help (checked 2026-09-07).
- [App-server account observations](https://learn.chatgpt.com/docs/app-server), [Codex usage](https://learn.chatgpt.com/docs/pricing), and [xAI reasoning](https://docs.x.ai/developers/model-capabilities/text/reasoning).
- Pinned installed OpenCodex source identified by `global/opencodex/dependency.json`; existing [subscription integration](../../../docs/subscription-models.md).
