## Why

The globally installed harness adds repeated diagnostic context and completion interruptions to ordinary Codex work, while the user reports exhausting a weekly ChatGPT allowance in a day. Existing logs confirm substantial hook traffic and expensive long Astra sessions, but do not establish a precise conversion from individual token sources to weekly quota.

## What Changes

- Make ordinary development without hooks the durable default, beyond the current suspension. Keep all global hooks suspended throughout this change, covering new sessions and cached handlers with recoverable state and global verification. A narrowly scoped exception requires a concrete unmet need, no adequate simpler native mechanism and demonstrated benefit; no hook is required merely to finish this change.
- Exclude routine instruction reinjection, broad repeated diagnostics and Stop-driven rechecking or continuation from the normal workflow. Potential exceptions such as a precise pre-operation constraint, cleanup of owned session resources or a local completion notification are examples to assess only when needed, not features to implement now. Successful hooks must add no model context, and silent execution must still justify its latency and maintenance cost.
- Establish a bounded, reproducible accounting of model responses, cached/uncached input, output/reasoning, hook text, continuations, delegation/rework and time to an accepted result. Reuse existing logs and outcome checks before spending on new experiments.
- **BREAKING:** automatic LSP becomes optional and eligible for restoration only after evidence of better effectiveness, quality and completion time than the hooks-off alternative. Retiring some or all LSP integrations is an accepted outcome when that benefit is absent or unproven.
- Prefer Serena's existing navigation, editing and file-diagnostics capabilities over a separate harness language-server stack when they cover the accepted needs. Start with existing explicit tools and project-native checks; evaluate ready-made inline diagnostics only when useful. Keep or add custom diagnostics only for a demonstrated important gap, after simpler supported configuration and native checks have been assessed. API availability alone does not establish freshness, scoped work or benefit.
- Include a workflow that prefers suitable Serena edits and, if beneficial, returns minimal diagnostics with a completed edit batch, potentially without native hooks. Evaluate actual edit coverage and total accepted-result cost; do not impose a 99% tool-usage target or force generators, unsupported operations and recovery through Serena.
- **BREAKING:** any restored automatic LSP may run ONLY for actual content modification or creation of supported files, and ONLY for those files. Reads, unchanged commands, deletion-only operations, Stop/SubagentStop, startup baselines, previous dirty state, unrelated files and pending-work retries cannot trigger analysis.
- If retained, deliver only bounded, relevant changes in diagnostics with per-file/revision deduplication. Preserve source integrity and honest uncertainty without automatic completion loops.
- Require the necessary minimum information from every automated interface and input path in the kit: silent no-ops, concise actionable failures or changed results, deduplicated feedback and explicitly retrievable detail. A size ceiling is not a target to fill.
- Evaluate excessive Astra reasoning, repeated context loading and ineffective delegation within existing Astra-only OpenAI / preferred Grok subscription constraints. Reuse the owning Grok stability change instead of duplicating its implementation.
- Deliver the selected smaller configuration through the global installation lifecycle. Updating, restarting or completing this change must not silently restore rejected hooks or LSP.

## Capabilities

### New Capabilities

- `subscription-efficiency`: evidence-based attribution, bounded comparisons, selection of useful capabilities and economical model/context/delegation behavior.

### Modified Capabilities

- `automatic-lsp-diagnostics`: replace broad automatic reconciliation with strictly mutation-scoped, conditional diagnostics and quiet completion.
- `global-code-tools`: make LSP retention conditional on demonstrated benefit, including complete retirement and honest capability discovery.
- `linked-global-kit`: persist hook suspension and the accepted capability selection across global installation, update, recovery and new sessions.

## Impact

Owning paths include `global/hooks.json`, `global/harness.config.toml`, `tools/hook.ps1`, `tools/lsp/`, the code-tool registry/installer and existing outcome/delegation evidence helpers. Machine-local suspension and backups remain outside Git. Global source links mean runtime changes affect other projects; verification uses owned neutral fixtures outside this checkout and preserves unrelated sessions and edits.

This change supersedes earlier unconditional automatic-diagnostic coverage and completion-reconciliation requirements where they conflict with the user's 2026-09-08 instructions. It coordinates with `stabilize-grok-delegation`, `accelerate-verified-delivery` and `adopt-project-memory-and-native-workflows`; it does not reopen their unrelated scope, require further `opencode-kit` runs, enable Fast mode or authorize paid API substitutions.
