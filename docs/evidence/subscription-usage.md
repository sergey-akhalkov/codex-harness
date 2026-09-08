# Subscription usage from existing local rollouts

[Documentation map](../README.md) · [User decision](../project-decisions.md#расход-подписки-и-условная-автоматизация) · [Optimization change](../../openspec/changes/archive/2026-09-08-reduce-subscription-waste/proposal.md)

Verified 2026-09-08 using Python 3.13.14 and the existing native CLI 0.153.3/0.153.4 rollouts. No model experiment was run. The installed-source reporter works outside this checkout. Its 34 accounting tests and 32 outcome/delegation consumer tests passed. Commands and global invocation are in [agent delegation](../agent-delegation.md).

Observed native JSONL accounting only. Token counts are not weekly quota
percentages, billing, or proof that a later task caused a reset-window change.
Reasoning output is included in output. Cumulative thread snapshots are not
summed with per-response deltas. Raw logs, identities and input hashes stay
in host-private evidence.

| Category | Count / tokens | Notes |
| --- | ---: | --- |
| CLI versions | 0.153.3, 0.153.4 | Native session_meta |
| Threads supplied | 227 | Explicit inputs only |
| Distinct response IDs | 13550 | Counted once |
| Thread input | 1404424292 | Last cumulative snapshot per thread |
| Cached input | 1348768512 | Included in input |
| Thread output | 7884526 | Includes reasoning |
| Reasoning output | 3331470 | Not added again |
| Reconciled total | unknown | Unknown when coverage is partial or the series disagree; not a quota share |
| Cumulative snapshot sum | 1412308818 | Last snapshot per supplied thread; inherited history can overlap |
| Per-response delta sum | 1623159147 | Deduplicated stable response IDs; partial observed series, not added to cumulative snapshots |
| Response input / cached / uncached | 1613816784 / 1551928064 / 61888720 | Cached input is included in input |
| Automatic context occurrences / chars | 308 / 1090158 | Recognized hook/subagent/internal-context markers; inherited or repeated occurrences are not deduplicated findings |
| Ordinary turns started | 655 | Not counted as continuation |
| Commentary messages | 2266 | Ordinary status; not continuation |
| Continuation notices | 25 | Interrupt/timeout/resume trigger text |
| Actual continuations | 18 | Distinct resumptions correlated with the next later turn before an ordinary user request; heuristic, not causal proof |
| Missing children | 13 | Spawn recorded, rollout not supplied |
| Compacted windows | 133 | Compaction envelopes are not extra responses; cumulative inheritance may overlap |

## Model / effort / project

| Role | Model | Provider | Effort | Project | Elapsed s | Responses | Partial |
| --- | --- | --- | --- | --- | ---: | ---: | --- |
| child x8 | gpt-6-astra | OpenAI | high | codex-harness | 26437 | 650 | yes |
| child x1 | gpt-6-astra | OpenAI | high | levels | 8 | 1 | no |
| child x24 | gpt-6-astra | OpenAI | high | pmac-emulator | 18408 | 574 | yes |
| child x1 | gpt-6-astra | OpenAI | max | levels | 23 | 1 | no |
| child x1 | gpt-6-astra | OpenAI | max | pmac-emulator | 550 | 11 | no |
| child x1 | gpt-6-astra | OpenAI | unknown | codex-harness | 3272 | 50 | yes |
| child x1 | gpt-6-astra | OpenAI | unknown | pmac-emulator | 3643 | 44 | yes |
| child x20 | gpt-6-astra | OpenAI | xhigh | codex-harness | 144535 | 2427 | yes |
| child x1 | gpt-6-astra | OpenAI | xhigh | levels | 12 | 1 | no |
| child x19 | gpt-6-astra | OpenAI | xhigh | pmac-emulator | 103374 | 2056 | yes |
| child x5 | gpt-6-astra | OpenAI | xhigh | workspace with spaces | 526 | 35 | yes |
| child x1 | unknown | unknown | xhigh | codex-harness | 41791 | 13 | yes |
| child x1 | unknown | unknown | xhigh | pmac-emulator | 592 | 7 | yes |
| child x3 | xai/grok-4.6 | xai | high | codex-harness | 1943 | 48 | no |
| child x1 | xai/grok-4.6 | xai | high | fixture | 19 | 2 | no |
| child x26 | xai/grok-4.6 | xai | xhigh | codex-harness | 13287 | 308 | yes |
| child x9 | xai/grok-4.6 | xai | xhigh | fixture | 306 | 10 | yes |
| child x2 | xai/grok-4.6 | xai | xhigh | mixed | 501 | 22 | no |
| child x20 | xai/grok-4.6 | xai | xhigh | pmac-emulator | 11224 | 266 | yes |
| root x4 | gpt-6-astra | OpenAI | low | neutral | 202 | 23 | yes |
| root x1 | gpt-6-astra | OpenAI | unknown | mixed | 327 | 12 | yes |
| root x1 | gpt-6-astra | OpenAI | unknown | pmac-emulator | 1792 | 43 | yes |
| root x28 | gpt-6-astra | OpenAI | xhigh | codex-harness | 203897 | 2715 | yes |
| root x1 | gpt-6-astra | OpenAI | xhigh | direct | 130 | 5 | no |
| root x11 | gpt-6-astra | OpenAI | xhigh | fixture | 1053 | 58 | yes |
| root x1 | gpt-6-astra | OpenAI | xhigh | levels | 110 | 6 | no |
| root x1 | gpt-6-astra | OpenAI | xhigh | mixed | 388 | 20 | no |
| root x17 | gpt-6-astra | OpenAI | xhigh | pmac-emulator | 185974 | 4032 | yes |
| root x2 | gpt-6-astra | OpenAI | xhigh | project | 535 | 17 | no |
| root x1 | gpt-6-astra | OpenAI | xhigh | projects | 520 | 15 | no |
| root x8 | gpt-6-astra | OpenAI | xhigh | workspace with spaces | 881 | 56 | yes |
| root x1 | unknown | unknown | unknown | unknown | 0 | 0 | yes |
| root x1 | xai/grok-4.6 | xai | high | fixture | 36 | 2 | no |
| root x3 | xai/grok-4.6 | xai | xhigh | workspace-187f5cdc | 453 | 13 | yes |
| root x1 | xai/grok-4.6 | xai | xhigh | workspace-2e1874bf | 286 | 7 | no |
| missing-child | unknown | unknown | unknown | unknown | unknown | 0 | yes |

## Limitations

- Partial records stay explicit; missing usage is null, not zero.
- Concurrent parent/child elapsed time is not attributed to one task.
- Quota snapshots from different reset windows are incomparable.
- Cumulative snapshot sums and deduplicated response deltas are separate series. Their disagreement is unresolved; neither is a reconciled complete total when coverage is partial.
- Automatic-context counts are recognized marker occurrences, not unique findings.
- Actual continuations are a timestamp heuristic: an intervening ordinary user request breaks correlation, and two triggers sharing one resumed turn count once.
- Source transcripts, credentials and absolute paths are omitted.
- Private source count: 227 hashed inputs retained only in host evidence.

Warning codes: concurrent_or_overlapping_elapsed, cumulative_usage_decreased, forked_or_compacted_history, interrupted_turn, missing_child, missing_reasoning, missing_response_id, missing_usage, mixed_model_attribution, mixed_project, mixed_reasoning, reasoning_not_included_in_output, unsupported_or_missing_model.

Input selection: 227 byte-matching inputs were recovered from the earlier 235-input hash manifest and frozen privately before this report. 8 original hashes could not be recovered; newer/live replacements were not silently substituted. This limits period coverage and comparisons with preliminary counts. The private input/path/hash ledger and complete JSON are at `~/.codex/harness/verification/subscription-usage/accepted-09cb135203cc4bf5a6b6c54534adc4fe/`.

The prior draft incorrectly treated shared `session_meta.session_id` as another thread identity and dropped native child usage. The final parser uses `session_meta.id`, distinguishes recorded inherited identities, and preserves unknown/conflicting data. Conflicting copies of response usage invalidate that response in every input order. Continuation correlation excludes a later unrelated user request and counts one resumed turn once.

The two token series disagree. This report therefore gives no reconciled account-wide total, no effort-setting comparison and no weekly-quota saving. Partial records, inherited counters, concurrent periods and unknown child contributions remain explicit. Reasoning fields exceeding output in native records are flagged; reasoning is never added again to output.
