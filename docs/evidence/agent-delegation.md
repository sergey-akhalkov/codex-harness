# Agent delegation acceptance, 2026-09-07

Status: implementation and global acceptance complete; all nine items in the
[task list](../../openspec/changes/adaptive-agent-delegation/tasks.md) are closed.
Runtime: Codex CLI 0.153.4, OpenCodex 2.44.0,
PowerShell 7.6.5, Python 3.13. No paid API credentials were introduced.

## Verified configuration and recovery

- Native consumer: 19 assertions passed, including actual global policy loading,
  base configuration preservation, project precedence, links and live source edits.
- Global core Check reports Connected, 12 links, six skills and three permanent
  Astra agents. Grok is supplied by its separate conditional subscription link.
- Usage helper: 17 deterministic tests passed, including CLI consumption outside
  the checkout, cumulative deduplication, missing/corrupt data, model attribution,
  duplicate conflicts and refusal to overwrite an input rollout.
- Live management GET confirms search backend xAI/model Grok 4.6 and vision disabled
  with explicit Astra model. Existing local admin authorization was used solely on
  loopback; no credential values were included in evidence. Search quality and
  image interpretation are not established by this settings check.

The user reported a session disconnection. The managed service result for PID 780
records `memory-limit`, exit 125, peak job commitment **852,525,056 bytes**, limit
**805,306,368 bytes**, after **3,471,245 ms**. Restoration then exited 0, and the
restarted client used native OpenAI with Grok absent. One independent implementation
slice was completed by the named Astra high reserve in that real unavailable state.
Its thread was `01a0794d-01bb-7c40-821d-42ee7c397bda`.

The service allowance was raised to **2048 MiB**, retaining 768 MiB for short
administrative commands and OAuth. Around 7.2 GiB of RAM was free before recovery.
The already stopped integration was reconnected through the installer; PID **6532**
became ready. This provides headroom, not proof that growing allocations or a leak
are fixed. The 256 MiB application retention budget is distinct from total Bun
memory. Failure remains contained, visible and recoverable; there is no infinite
restart loop. Original private result:
`~/.codex/harness/subscriptions/runs/20260906T233411-c6c76fe3a8ee42ef9eacc0cd0005df3e.result.json`.

## Actual Grok review

The initial live review failed acceptance although the shell command succeeded:
Grok awaited a nested tool without `text(result)`, saw only `done`, and could not
return the fixture marker. The parent correctly reported missing evidence rather
than inventing the finding. A two-line instruction in the universal middle
definition corrects the tool-output handling; it does not weaken acceptance.

The corrected run passed the complete existing read-only consumer scenario:
one fresh middle, exact `xai/grok-4.6`, actual shell evidence, marker and 41/42
discrepancy returned to the parent, unchanged fixture and no generated registrations.
Parent: `01a07957-10ce-7b50-b9f6-78282a8f65eb`;
child: `01a07957-969a-7723-87d7-8e33490530b5`.
Private report: `%TEMP%/codex-subscription-consumer-da080340047d4dee872b31b2b7418a34/report.json`.
The failed attempt is retained at
`%TEMP%/codex-subscription-consumer-1af2df1496694c3a91ebf5ddffca4c42/`.

## Matched implementation and parallel work

Runner: [tests/agent-delegation.py](../../tests/agent-delegation.py), opt-in native
sessions through the installed global launcher, outside this checkout. Each parent
and its descendants use a Windows Job with 2048 MiB and a 600-second deadline.
Both runs received the same module contracts and immutable four-test acceptance:
parse a bounded selection of integers/ranges and merge closed integer intervals.
The mixed run deliberately forces two workers to measure the overhead; it is not
the policy recommendation for two short helpers. Both final implementations passed.

| Observation | Direct Astra xhigh | Astra xhigh + two Grok xhigh |
| --- | ---: | ---: |
| Native process elapsed seconds, including workers/review/rework | 135.814 | 393.745 |
| Parent OpenAI input tokens | 94,997 | 538,490 |
| Of those, cached input | 78,464 | 507,136 |
| Input minus cached input | 16,533 | 31,354 |
| Parent OpenAI output tokens | 2,790 | 4,683 |
| Of those, reasoning output | 726 | 1,014 |
| Parent total tokens | 97,787 | 543,173 |
| xAI child usage | No xAI threads | Unknown for both children |
| Parent revision requests to workers | 0 | 2 |
| Parent shell commands | 4 | 11 |
| Immutable acceptance | Passed | Passed |

Reasoning is part of output, not an extra amount to add. The final cumulative native
snapshot is counted once per thread. Child rollouts establish model, effort and
parentage but contain no usable cumulative token snapshot in this runtime. Their
usage is `null`, mixed accounting is explicitly incomplete, and the known parent
cost must not be presented as the total cost of both subscriptions. Raw wall times
including supervisor startup were 137.039 and 394.820 seconds; the table consistently
uses the contained process interval.

Direct parent: `01a07957-0941-7803-8546-15dd140e9efc`.
Mixed parent: `01a07959-1ff7-7b92-a17c-dabe2f459e81`.
Mixed workers:

- `01a07959-d46d-70c0-9dd7-6a500640668b`: exclusive `selection.py`,
  lifetime 00:52:05.415–00:57:32.791 UTC.
- `01a07959-d528-7781-a26f-3c89680e324e`: exclusive `intervals.py`,
  lifetime 00:52:05.599–00:54:59.960 UTC.

All executed child contexts identify `xai/grok-4.6` / `xhigh`; both completed.
Lifetimes overlap, which proves concurrent outstanding work, not parallel hardware
execution. Manual inspection of child write commands confirms only their respective
module destinations. Parent commands read files and execute checks; no parent
implementation writes or native file-change items were found. The parent kept
workers responsible for corrections and independently reran acceptance, 600 seeded
randomized cases and targeted boundaries. The direct parent also ran additional
checks. Additional checks chosen by the parents differed; only the base acceptance
and initial module contracts were matched.

The two revision requests addressed integer subclasses and 5000-digit values.
Worker tool syntax/quoting mistakes also consumed time before successful writes.
Those costs are retained in the measurement. Forced delegation was about **2.9×
slower**, with about **1.9× more uncached OpenAI input** and **1.68× more OpenAI
output**. No saving or quality advantage was demonstrated for this workload.
Independent review traffic overlapped part of the experiment; this was a shared
host/account, not an isolated statistical performance study.

The production policy was corrected to batch related routine edits, avoid splitting
short helpers, state meaningful input boundaries upfront and avoid frequent status
polling when only waiting remains. These instructions are globally connected;
revalidation uses retained results, without another paid comparison. Their effect
on larger real workloads remains unmeasured. No general speed/quality improvement
or exact weekly quota saving is claimed from configuration or delegation count.

Private evidence root: `%TEMP%/codex-agent-delegation-t5cky0em/`.
Each case retains request, process result, native events, final response, acceptance
output and `report.json`; `revalidated.json` records all three accepted scenarios.
Detailed tool logs remain in their native rollouts, not in reusable configuration.

## Astra levels and real reserve use

The same external native runner completed the synthetic level-binding scenario
in 115.317 seconds. All contexts, not just the final one, match their definitions:

| Agent | Model / effort | Native child |
| --- | --- | --- |
| middle_backup | gpt-6-astra / high | 01a0795f-aa35-7ab2-aaf4-22bc90d333c8 |
| senior | gpt-6-astra / xhigh | 01a0795f-edb5-7b91-9c44-5cb8948c41ea |
| principal | gpt-6-astra / max | 01a07960-4f27-7781-98c2-9e808169b482 |

The parent checked returned arithmetic, a counterexample and a lost-update
interleaving. This single max call verifies invocation and response, not superior
reasoning quality or the production blocker threshold. Parent usage: 117,771 input,
103,168 cached input, 864 output (including 115 reasoning), total 118,635. All three
children have unknown native usage; aggregate accounting is partial.

Separately, the actual service outage described above caused a named Astra high
reserve to deliver the usage helper and its deterministic tests while Grok was
absent. Existing partial files were inspected before assigning the disjoint slice.
That is observed fallback under real unavailability; the level fixture merely
simulates it. Available Grok was subsequently used for both implementation and
independent review. Finished children were closed.

## Independent review and corrections

A fresh read-only Grok 4.6 xhigh review finished with exit 0. Private evidence:
`%TEMP%/codex-delegation-review-e17b66e7bde1439aaf716a8e1b1c1c7b/`.
Its findings were assessed against actual native behavior:

- Missing permanent `middle` was reported as a discovery defect. Rejected: it is
  intentionally delivered through the conditional subscription link, and actual
  named parent-to-middle execution passed. `subagentModels` is not this CLI's
  native agent-file catalogue. Duplicating it permanently would break disconnect.
- Checking only the final model could miss an earlier wrong binding. Corrected:
  verify every executed context, child completion and parent identity, reject
  parent native implementation patches; inspect shell writes for this acceptance.
- Failed runs could omit cost evidence and rework was a fixed label. Corrected:
  always retain available rollouts/usage with failure status, mark missing threads
  incomplete, and count actual completed `send_input` events. Four offline
  regression tests cover failed process, missing thread, missing child and earlier
  wrong binding. All passed; retained native scenarios revalidated successfully.
- Empty provider buckets and conservative `partial` flags could be misread.
  Documented: zero observed threads is not a quota balance; unknown data is null.
- Overlapping lifetimes do not establish hardware parallelism. Claims use the
  narrower observation above. The two-thread cap is an intentional initial bound.

## Global lifecycle and final checks

Actual isolated installation passed **40 checks** with separate homes, port and
scheduled task: install, repeat install, ready beyond 120 seconds, exact task stop,
native restoration, task restart, disconnect and reconnect. After each restoration,
the permanent link and all three Astra definitions remained intact while the
conditional subscription role followed service state. Every service request had
2048 MiB; short administrative jobs retained 768 MiB. No credentials were imported.
All eight recorded global configuration/auth preservation checks passed, and the
exact fixture task/job were cleaned up. Private root:
`%TEMP%/codex-subscription-isolated-2343ba9d07bd479eaf80fce36c03fbee/`.

Global read-only core and subscription Check, invoked outside the checkout, report
Connected and ready. The recovered live Bun remained PID 6532 after all acceptance
work; at the final service check its private bytes were about 572 MiB. This is a
point observation, not a sustained-load memory bound or leak diagnosis.

Relevant checks include native consumer (19 assertions), subscription source
validation (7 cases), deterministic subscription routing (139 assertions), usage accounting (17
tests), evidence failure handling (4 tests), live Grok consumer, three native
delegation scenarios and the isolated lifecycle. Automatic whole-workspace language
diagnostics include unrelated pre-existing findings; a clean whole-workspace static
analysis is not claimed. Scoped Python/PowerShell parsing, TOML/JSON parsing, 68
local link targets in 11 documents and strict OpenSpec validation passed before
task closure. A fresh global native prompt inspection also loaded the final
batching, Grok priority and Astra-only policy without a model request.
