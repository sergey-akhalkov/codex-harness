## Context

See proposal.md. Codex CLI 0.153.4 uses the linked harness profile and OpenCodex 2.44.0. The original child `01a0808f-8ced-7661-81b4-f1566f577a73` completed two turns with null final after yielded exec calls. The isolated child `01a080ad-4158-73a2-aa9b-557485b64812` reproduced premature final delivery after a three-second command yielded: it promised to wait without retrieving the result. Another read-only middle assignment completed successfully without intervention.

## Goals / Non-Goals

Correct continuation and parent recovery within the existing native agent integration. Preserve model, subscription, shared proxy availability and concurrent edits. No general scheduler, model replacement or unrelated diagnostics work.

## Decisions

Start at the smallest demonstrated boundary: the middle tool/continuation contract and parent outcome classification. Explicitly distinguish unified-exec cell IDs from shell process session IDs; require the matching wait mechanism and final evidence. If the same controlled probe still fails, inspect actual routed tool availability and repair that owning compatibility mechanism before claiming completion. Merely lengthening waits would hide the trigger and is not acceptance.

The prompt-only candidate failed the same trigger. Pin Grok 4.6 to the supported `modelAdapters` Chat override: the provider-wide Chat default was overridden by the OAuth model's Responses default. Preserve `xai/grok-4.6`, `xhigh` and Grok OAuth. Keep code mode; the tested direct-shell alternative exposed incompatible float arguments and is not deployed. Use only the required string `cell_id` for the outer wait, omitting optional numeric fields.

Enable the pinned proxy's existing `emptyCompletionRetry` and xAI `terminalContinuationGuard`. The former retries an empty completion once and reports an error if recovery fails; it does not replay exposed tool calls. The latter permits one correction of a detected status-only terminal. Its heuristic is limited and cannot guarantee arbitrary model compliance. These are bounded responses to an observed output defect, not periodic parent reminders. Exercise the actual pinned implementations offline and retain native acceptance separately.

Keep coordination in the existing global profile and explanations in agent-delegation.md. Do not turn a 30–60 second status interval into a deadline, and do not add a rigid retry ladder. An empty/status-only final permits evidence-based correction or direct completion, not an inferred quota failure.

Opaque state is protocol data, not an additional task artifact the parent must read. Visible errors about invalid encrypted state remain errors; its presence alone has no outage meaning.

The recorded encrypted-content failure followed native `resume_agent` restoring a former Grok child with `gpt-6-astra`. Since this tool has no model override, a restored parent must inspect partial work and use a fresh named middle with a visible handoff. An old transcript's model does not establish the resumed binding. This avoids incompatible state replay; it does not claim to repair upstream Codex's resume implementation.

Use the native bounded process runner for new outside-session checks where needed. Preserve original and reduced transcript identities privately, and retain a compact regression oracle which rejects a promised wait or invented result. Include a delayed failure and dependent useful work. No model-backed broad benchmark is required.

## Risks / Trade-offs

- Prompt guidance cannot force model compliance → exercise actual delayed continuation and dependent work, and state the sample limits.
- The current session may cache agent definitions → verify a newly launched global consumer as well as current-session behavior.
- Shared proxy changes can disconnect the control session → use config/role links without stopping the proxy; any necessary transport repair requires isolated validation and safe activation.
- Concurrent unrelated changes invalidate broad baselines → record relevant source hashes and use scoped checks.

## Migration Plan

Update repository-owned linked sources and verify native discovery outside the repository. Save pre-edit versions privately for scoped rollback. Preserve failed and partial receipts. Do not claim a runtime fix from parser/configuration checks alone.
