# Observation-pack notes

Measurement and acceptance evidence for this change, produced by the acceptance
suite [rtk_adapter.rs](../../../crates/codex-harness/tests/rtk_adapter.rs).
Only local fixture bytes and avoided reruns are recorded: no token, session or
weekly-quota effect is claimed, and no model task was involved.

## Byte accounting (task 3.2)

Fixture: `byte_accounting_for_bounded_recall_and_footer_overhead`. A
3000-line allowlisted command double emits a byte-stable 483,000 B of original
stdout (161 B per line). The staged adapter compresses it and packs the retained
bytes into an owned `CODEX_HOME`.

```powershell
cargo test -p codex-harness --test rtk_adapter -- --nocapture byte_accounting
```

| Quantity | Measured |
| --- | --- |
| Whole-file raw re-read of the retained archive | 483,000 B |
| Footer line added by packing | 152 B |
| Bounded recall, default window (lines 1-200 of 3000) | 33,405 B |
| Bounded recall, second window (`--offset 201`) | 33,515 B |
| Source-command reruns while recalling | 0 |

The raw figure is exact. The footer and recall figures move by a few bytes with
the handle's process-id digits (the handle appears twice in the footer and twice
in the recall response) and with the length of the staged source path repeated
in the provenance header; treat them as ±10 B on one machine. One 200-line
window is about 14.5 times smaller than the whole-file re-read, and both windows
were served after the source command double was deleted, which is the
avoided-rerun evidence: recall reads the packed archive instead of executing the
command again.

## Acceptance matrix (tasks 3.1-3.2)

| Spec behavior | Acceptance test |
| --- | --- |
| Handle and digest on the compressed path, raw locator unchanged | `compact_mints_a_handle_and_recall_returns_the_exact_window` |
| Explicit disable, unsupported form, filter failure, oversize, terminal stdout emit no handle | `bypass_paths_never_present_a_handle`, `terminal_stdout_bypasses_compression_and_retention`, `compact_without_rtk_and_disable_stay_raw`, `filter_failure_and_oversize_stay_raw_without_locator` |
| Pack failure falls back to the unchanged compact output | `compact_falls_back_to_todays_output_when_packing_is_unavailable` |
| Exact window with provenance and line coordinates | `compact_mints_a_handle_and_recall_returns_the_exact_window` |
| Limit clamp at 2000 lines and the 256 KiB emitted bound with truncation marker | `recall_clamps_the_requested_window_and_marks_truncation` |
| Evicted or unknown handle, usage errors and exit codes | `retention_evicts_the_oldest_observation_and_cleans_orphans`, `recall_reports_usage_and_unknown_handles_with_exit_codes` |
| Digest mismatch fails closed with no content | `recall_withholds_content_when_the_digest_does_not_match` |
| Retention outlives the creating process and the raw keep window | `packed_observation_outlives_its_process_and_the_raw_keep_window` |

Retention and failure behavior were observed through the adapter's own entry
points on this fixture only; the installed consumer route is verified by the
delivery task (4.2), not here.

## Installed consumer verification (task 4.2)

Delivered through `install --token-workflow-only` (source identity
`7a784680826c328f2b15e734c683610ac856a4016691a873cc4961161ccebedf7`,
`check --token-workflow-only` passing). From a temporary directory outside the
kit checkout, a real compact of `git -C <kit> log -n 400` emitted 464 B of
compressed output where the raw log is 25,443 B, with both the unchanged
`[rtk raw: ...]` locator and the `[rtk pack: ...]` handle line. Recall of that
handle with `--offset 5 --limit 3` returned the exact numbered window with a
`digest: verified` provenance header and a next-window hint (exit 0); an
unknown handle failed with the rerun/raw remedy (exit 2).

This route found two defects the fixture suite could not see, both fixed before
delivery: the pinned `rtk.exe` prints a `[rtk] No hook installed` startup notice
to stderr in the kit configuration, which disabled compression everywhere
outside fixtures (only that banner is ignored now); and a non-shrinking filter
output packed an observation whose handle was never presented (packing now
happens only on the emitted compact result).
