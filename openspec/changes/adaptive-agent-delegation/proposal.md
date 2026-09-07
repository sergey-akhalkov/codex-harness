## Why

The user wants more completed, verified work from their existing ChatGPT and SuperGrok Heavy subscriptions: lower ChatGPT consumption, useful parallelism, and autonomous escalation without delegation theatre. The current integration proves subscription routing but supplies only a task-specific Grok reviewer and has no common selection policy or comparative outcome evidence.

## What Changes

- Deliver universal capability levels: preferred Grok 4.6 xhigh middle, subscription-backed Astra high middle reserve, Astra xhigh senior, and rarely used Astra max principal. All OpenAI assignments use only the Astra family, including auxiliary calls. Tasks and relevant skills determine the activity; model assignments remain replaceable configuration.
- Activate a concise global policy that prefers Grok for worthwhile bounded delegation, permits direct senior work when more efficient or necessary, and autonomously switches on observed unavailability or escalates genuine reasoning blockers. Completion and quality take precedence over quota minimization.
- **BREAKING**: retire `grok_reviewer` and replace its current consumer references with the universal middle level. Historical evidence remains historical.
- Bound parallelism and context transfer, preserve task ownership and meaningful verification, prevent automatic delegation trees, and distinguish instructional effort budgets from enforceable runtime limits.
- Make search/vision auxiliary OpenAI usage explicit and avoid hidden OpenAI sidecar consumption by the preferred middle path.
- Add reproducible native consumer scenarios and comparative evidence including parent and child usage, elapsed time, acceptance results and rework. Report observed limits without inventing weekly quota savings from token counts.
- Supply and verify the capability globally through the existing linked installation lifecycle, preserving unrelated configuration, credentials, work and the running control channel.

## Capabilities

### New Capabilities

- `agent-delegation`: Global capability levels, skill-independent task assignment, preferred subscription routing, autonomous fallback/escalation, bounded collaboration and evidence of end-to-end efficiency.

### Modified Capabilities

None in the current main spec tree. This change explicitly supersedes the `grok_reviewer` delivery name in the completed, not-yet-archived `connect-subscription-model-routing` change; it preserves that change's subscription authentication, exact-model transport and recovery requirements.

## Impact

Linked `global/harness.config.toml`, universal definitions under `global/agents` and the subscription-owned agent directory, OpenCodex sidecar configuration, source validation, existing subscription consumer checks, focused delegation verification/measurement helpers, installation documentation and the durable decision record. Retain official Codex CLI 0.153.4 and pinned OpenCodex 2.44.0; no new production orchestrator, paid API account or daemon is planned.
