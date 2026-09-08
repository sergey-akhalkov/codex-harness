# Native context contracts: bounded investigation

2026-09-08. **Both assigned probes pass: autonomous task 1.1 and adopt task 2.1.** The accepted ordinary CLI runs demonstrate current-session skill delivery, manual and automatic mid-turn compaction, resume, and a separate active Astra context pilot. See the resumed-run section for acceptance and the auxiliary-model violation found and corrected in earlier attempts. Main owns integration and archive; this sidecar does not implement the entire changes. All native run handles are closed.

In the initial pass, the real Astra pilot completed a native CLI turn, but its file tools could not start. Its independent output check failed despite native exit 0. A model-free comparison reproduced the same failure with packaged PowerShell and succeeded with inbox PowerShell. Subsequent cause-corrected attempts are appended, not substituted for those failures.

## Identity, scope and references

- Installed `codex-cli 0.153.4`, native binary SHA256 `444A3F0008050605CAE73CD9B7A2DCAC61294062DFAAB56DD20430FD6498518B`. The absolute executable is recorded in each `report.json`; it is the native executable behind the installed npm wrapper, discovered from the live kit's installation metadata.
- Requested model `gpt-6-astra`, provider `openai`, existing subscription route `http://127.0.0.1:10100/v1`, ChatGPT auth, no API key. Main pilot rollout `turn_context.model` is `gpt-6-astra`. This alone did not cover auxiliary requests: resumed TUI diagnostics exposed automatic Luna task-title generation. No service restart or provider change was performed.
- Initial repository HEAD `dfc2d6af49f2da949b2b1b0ed549a33c62d1eb4b`; shared tree dirty. Reports pin probe/helper hashes, so HEAD alone is not the build identity. Earlier probe revisions have recorded hashes and transcripts, not a complete source snapshot per attempt.
- OpenSpec `status --change NAME --json` and `instructions apply --change NAME --json` were read for both changes, followed by their context files. Skills used: openspec-apply-change, openai-docs, project-verification; reproduce-regression for the concrete subprocess failure. Local CLI/schema/source inspection preceded official web documentation, following the controlling task instruction.
- MCP discovery and independent context verification matched CBM project `D-home-sergey-akhalkov-codex-harness` and Serena project `harness` with Python support. CBM indexing was confirmed current at initial navigation. Its tools/docs exclusion required direct source reads for these helpers; no negative graph result was used as source-absence evidence. No stale graph navigation followed edits.

The official [hooks contract](https://learn.chatgpt.com/docs/hooks), fetched on this date, documents JSON `additionalContext` for SessionStart, UserPromptSubmit and PostToolUse. SessionStart sources include startup, resume, clear and compact. Automatic root compaction delivers the compact SessionStart before the immediate next model request, including mid-turn; `continue: false` can halt it. PreCompact/PostCompact supply lifecycle observations, not additionalContext. These are documented contracts, not demonstrated results of this fixture.

The [models documentation](https://learn.chatgpt.com/docs/models) describes experimental context eligibility for Plus/Pro/Pro Lite. Native account inspection returned ChatGPT Pro. This meets a documented account prerequisite; it does not prove a server-side grant for the routed Astra model. The [configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference) documents compaction thresholds/scope and Windows sandbox modes. Setting a threshold cannot establish that a continuation happened.

## Initial evidence and acceptance gaps (superseded below)

| Contract | Observed | Acceptance still missing |
| --- | --- | --- |
| Installed parser | `features list -c features.context_management.experimental_mode=true` exited 0; false also parsed in skills attempts | Parser acceptance alone says nothing about activation |
| Effective setting | Native `config/read`, using the same owned home, returned `features.context_management.experimental_mode: true` for the pilot | This inspection uses App Server; runtime consumption remains unproved |
| Eligibility | Native `account/read`: `type=chatgpt`, `planType=pro`; identifying fields removed before recording | Astra runtime eligibility/activation is not established by the account tier |
| Context runtime | Actual `codex exec --json` completed one Astra turn, thread `01a080ae-741a-7f30-a3a8-36ba346e963f` | No native compaction record or experimental runtime activation evidence; no continuation demonstrated |
| Earlier invariant | The model repeated that red candidates remain ineligible | It could not read candidates or select `winning-allowed`; repeating the rule before any continuation is insufficient |
| Hook installation/trust | Native ConPTY accepted the five owned hooks; independent `hooks/list` showed all five trusted in three skills attempts | No hook event receipt or marker delivery was obtained |
| Skill discovery/create/update | Staged fixture carries independent revision tokens and expected answers 58/90 | No actual new/current skill read or correct answer observed in a CLI session |
| SessionStart lifecycle | TUI setup and an interrupted rollout were observed | Startup/resume/manual compact/actual automatic mid-turn delivery and subsequent output all remain unproved |

No claim that the installed CLI lacks these documented capabilities follows from the setup failures.

## Preserved initial attempts

All roots below are under `C:\Users\noilw\AppData\Local\Temp`. A root suffix denotes `native-context-contracts-SUFFIX`. Each model-enabled attempt retains `process-request.json`, `process-started.json`, `process-result.json`, worker stdout/stderr, `report.json`, owned config/inputs, and any terminal/rollout/trust output actually produced. Reports include UTC stages, full argv via request receipts, native/helper hashes and model route. Absent hook-event files mean no receipts were obtained; they are not successful empty results.

| Root suffix | Outcome and retained cause |
| --- | --- |
| `8367cfb5cd5c47a993adbc4f111b2715` | Static cache: identity, generated experimental App Server JSON schemas, official hooks/models/config-reference text, final attempt inventory and validation. The generated protocol schema has extensible config fields; it is not a typed proof of the experimental feature. |
| `860bb451c95f4f5e8d0c981bdd055cb3` | Skills/TUI worker 25936, exit 1, 37.769 s. Fixture searched spaced text while ConPTY split/removed spaces. Timeout before prompt. |
| `fc4e34cfabcd45878675ece62f1f0f9b` | Skills/TUI worker 15404, exit 1, 151.693 s. Hooks trusted, but prompt handling hit sandbox onboarding. Default admin setup failed; no completed model turn. An unintended setup selection occurred in this owned home. Later fixture explicitly specifies live-equivalent `windows.sandbox=unelevated` and rejects onboarding. |
| `aa51865e3f0f410389e112b234ae7e84` | Skills/TUI worker 23244, exit 1, 12.809 s. Own `ReadAllLines` hit sharing violation on the live rollout; terminal disposed. Interrupted session `01a080a1-dba6-71f1-b0ec-17d19d2a5caa`. Replaced with a shared-access snapshot reader. |
| `5cd3ee0c01ed4bab840d68425fa149a5` | Skills/TUI worker 1768, native PID 25364, exit 1, 154.082 s. Hooks trusted; prompt remained in composer while startup was settling. No completed turn. Former execution handle 71044 finished; this was the surfaced stalled probe. |
| `c45c038d86274727acd1c9faea0c4d6e` | Pilot with preliminary TUI hook review, worker 25040, exit 1, 53.168 s. Late hook-review dialog missed after folder trust; timeout before model call. Hook-review waiting corrected subsequently. |
| `08412d5cd32c4c5a9fe6d7a90b48fe49` | Independent pilot/exec, hooks disabled, compact threshold 8000 with `body_after_prefix`. Worker 18412 exited 0 after 65.139 s; native 6384 exited 0 after 52.718 s. One real Astra turn. Three tool-start errors, no compaction, wrong/missing task output. `oracle-checked.json` records output failure. |

At the initial handoff, TUI failures had received cause-specific fixture corrections, not success labels. That TUI revision (bracketed paste/readiness handling, native manual-compaction records, dynamic turn counts, explicit automatic case before resuming) had only syntax/opt-in checks; its later lifecycle stages were unexecuted. The resumed work below first resolved native shell selection before exercising those stages.

All six copied `home/auth.json` files were removed; `attempt-inventory.json` verifies their absence. Outer workers ended naturally with the codes above. Failed ConPTY cases disposed their owned terminal; no pending tool execution handle is being waited on. This is not a claim about unrelated shared processes. Runtime state and raw transcripts remain local and should not be published.

## Initial runtime failure and model-free comparison

The completed pilot returned `Eligible candidate id: undetermined. Latest context receipts: unavailable.` The native rollout records three `CreateProcessAsUserW failed: -1073283067` errors (hex `0xc0070005`) before file contents were available. The attempted executable was `C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.5.0_x64__8wekyb3d8bbwe\pwsh.exe`. Even the model's explicit `shell: C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe`, `login: false` call produced an error naming packaged PowerShell. Why that override was ineffective was not established.

Reduction receipts are in `native-context-contracts-reduction-1f88e0f097e44fd2914d3a8b12107d51`. Both valid arms used the same installed native executable, read-only/unelevated configuration in a fresh owned home, existing owned workspace, no auth/model call, 512 MiB job and 20 s deadline. Only the child executable differed:

```powershell
# cwd: the pilot's owned workspace; CODEX_HOME: the reduction's owned home
& $native sandbox -- $packagedPwsh -NoLogo -NoProfile -Command 'Get-Content -LiteralPath seed.txt'
& $native sandbox -- C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe -NoLogo -NoProfile -Command 'Get-Content -LiteralPath seed.txt'
```

`appx-pwsh-cwd-result.json`: PID 22788, natural exit 1 in 149 ms, original `-1073283067` signature. `inbox-powershell-cwd-result.json`: PID 19136, natural exit 0 in 463 ms, stdout exactly `17`. The shared temporary-home helper-alias warning appears in both arms, so that warning alone cannot explain the failure. This isolates an executable-dependent restricted-token startup failure, not a context-management refusal or a historical CLI regression.

Rejected reductions are retained too: initially adding `-C` caused both arms to exit 2 requiring `--permission-profile`, before sandbox execution (`*-request/result/out/err` without `-cwd`). Earlier `sandbox windows --help` incorrectly treated `windows` as the child command and failed with error 2 (file not found); that console result belongs to this sidecar transcript, not the original-failure reproduction. `sandbox --help` then established the installed Windows syntax. None of these invalid invocations proves the target runtime contract.

The pilot also logged WebSocket 426 from the existing route; a model turn nevertheless completed. No provider substitution or proxy repair was attempted, and this is not assigned as the file-tool failure cause.

## Initial files, commands and verification

- [Probe runner](../../tests/native-context-contracts.ps1): opt-in, owned home/workspace, native ConPTY and existing [RPC consumer](../../tests/consumer-rpc.ps1), existing [Windows job observer](../../tools/opencodex-process.ps1). Outer limit 540 s / 2048 MiB; exec limit 210 s. `status=observed` describes collection, never acceptance. Parent now propagates worker infrastructure failure.
- [Owned hook](../../tests/native-context-contracts-hook.ps1): creation/update procedure with revision receipts; JSON additionalContext and lifecycle logging. Never installed globally.
- [Independent oracle](../../tests/native-context-contracts-oracle.py): strict completed-JSONL parsing, native model/session/compaction observations, revision/answer checks and marker-to-output correlation. `--check-output` exits 1 for missing expected output. A passing output check would still require lifecycle ordering, actual skill reads and experimental runtime evidence for full acceptance.

Executed model probe families (the per-attempt request/report specifies exact revision, root and threshold):

```powershell
./tests/native-context-contracts.ps1 -RunModelProbes -Scenario skills -Transport tui
./tests/native-context-contracts.ps1 -RunModelProbes -Scenario pilot -Transport exec -AutoCompactTokens 8000
./.venv/Scripts/python.exe tests/native-context-contracts-oracle.py "$env:TEMP/native-context-contracts-08412d5cd32c4c5a9fe6d7a90b48fe49" --check-output
```

Final checks: both PowerShell files parsed with `System.Management.Automation.Language.Parser`; Python AST parsed; no-opt-in invocation returned SKIP; independent oracle rejected the real natural-exit-0 pilot with exit 1. These checks are saved in the static cache's `verification.json`. Automatic workspace diagnostics remained incomplete and included unrelated issues; no workspace-wide clean claim is made.

Initial handoff, superseded by the completed resumed work below: establish supported native consumer shell selection using an owned no-model check, preserving the packaged-shell counterexample. Then run bounded skills TUI and experimental pilot cases with real continuation. Neither settings nor account checks can substitute for those observations.

## Resumed run: shell selection and actual continuation

The installed native binary and hash remained unchanged. OpenSpec status/apply instructions were refreshed for both changes and the relevant source contexts reread. CBM project identity still matched; no stale graph queries were used. Serena reported `proxy source/runtime changed; restart this client`, so direct reads covered the affected scripts and installed runtime without restarting the shared client/service. Global hooks were intentionally empty during this pass and were not restored. The separately proposed global hook suspension and other agents' configuration edits were left alone.

**Verified shell selection:** exclude child PATH entries matching `(?i)\\WindowsApps(?:\\|$)` before starting native Codex. This selects the already-installed desktop `C:\Program Files\PowerShell\7\pwsh.exe`. No executable is installed or replaced, and parent/global PATH is unchanged. [The no-model comparison](../../tests/native-context-contracts-shell.ps1) first asks the native configured shell to report its actual executable and read owned `seed.txt`, then runs that selected executable via native `command/exec` with `readOnly` sandbox. The first operation is unsandboxed by its native schema and contains only those fixed read/identity commands; it is not represented as sandbox evidence.

| New root suffix (under the same host temp directory) | Exact observation |
| --- | --- |
| `shell-d4e34869a9c8417ebd6d48036ee43abc` | No-model selection succeeded: inherited PATH selected packaged PS7, filtered PATH selected desktop PS7. Both sandbox checks rejected the optional `outputBytesCap` field (`custom outputBytesCap is not supported with windows sandbox`). Retained as API-shape failure, not shell success. |
| `shell-78981da418b04e3b9064245efa98a4a9` | Removed only that optional field. Baseline reproduced `CreateProcessAsUserW: -1073283067`; filtered native-selected desktop PS7 returned stdout `17`, exit 0. Workers 19952/26092 exited 0 in 2.933/2.641 s. Full native RPC events and requests retained in each arm. |
| `9e67cd12188949a9ab8db9d20128eafc` | Corrected pilot/exec: worker 6612 exited 0 in 48.120 s, native 29140. Desktop PS7 read both inputs. Actual compaction at 11:35:48.927Z, then correct winner. Exec SQLite diagnostics were empty, so this alone did not establish experimental runtime activation or exhaustive auxiliary-model identity. |
| `6a73c6b5b44043a7ad5603472f2918f6` | Corrected skills/TUI shell: actual model turn, but all owned hooks exited 1. Worker 19420 exited 1 in 27.105 s because no skill file was created. This isolated a separate hook-command invocation defect. |
| `hook-command-a97c941a177649e1964b376134518674` | No-model reduction: a quoted executable without PowerShell's call operator produced ParserError `Unexpected token '-NoLogo'`, exit 1. Prefixing `&` returned valid SessionStart JSON and a receipt, exit 0. Native TUI success in the following run confirmed applicability. |
| `4fad07e8a0fc4052bd59f1e6ed466078` | Hook correction: worker 21336 exited 0 in 162.792 s. Native PID 25992 handled creation/update/manual/automatic compaction; PID 26176 resumed the same thread. Correct answers 58/90 and actual automatic SessionStart receipt delivery observed. A fixture counter erroneously counted manual `/compact`'s null-answer completion, causing prompt overlap; raw ordering was retained. Native auxiliary Luna title sampling also disqualifies an Astra-only acceptance claim. |
| `1034c3ed075d43648efc35bc0e3705cb` | Manual-counter correction. Worker 12496 exited 1 after 94.308 s because the sidecar stopped its verified owned native PID 29428 on discovering auxiliary Luna sampling; stop receipt records native executable/start identity. Partial creation/update/manual evidence retained. |
| `c0a8c79ed1e24918937a69956d3ff3e8` | Pilot/TUI: worker 29192 exited 0 in 51.352 s. Runtime sampling diagnostics listed `ContextManagement`; actual automatic compaction and correct final output observed. Auxiliary Luna title sampling means this is not the accepted Astra-only pilot. |
| `33aae5d7b5dd46dda2cec917919de0df` | No-model named-session check: worker 3724 exited 0 in 15.341 s; native PID 33132. `/rename Native context contracts` persisted that exact thread name before any model input; sampling-record count zero. |
| `615a7db9a5a14869859acda2ad0f02f8` | **Accepted task 2.1 pilot.** Named TUI, worker 13604 exited 0 in 58.505 s; native PID 1772 exited naturally 0. All native sampling records are Astra. Experimental runtime flag and post-compaction application are detailed below. |
| `8b9b787f78484c8488d5f290e547e127` | **Accepted task 1.1 lifecycle.** Worker 17208 exited naturally 0 in 177.520 s; native main PID 17768 and resume PID 31868 exited naturally 0. Handle 38506 closed. All native sampling records are Astra. |

New model runs snapshot their executed runner/hook/ConPTY/observer sources under `source/` and record hashes, exact outer arguments, timestamps and route. The oracle's original literal phrase check incorrectly rejected correct answers formatted as `Eligible candidate: ...` or `Candidate: ...`; the task did not require a fixed label. Its corrected affirmative-candidate check accepts those forms and still rejects the original real `undetermined` result and any chosen forbidden winner. This change does not alter the expected winning id or continuation criteria.

### Auxiliary model violation and correction

TUI SQLite diagnostics revealed an ephemeral `gpt-5.6-luna` sampling request with the fixed instruction to generate a short task title. This was an automatic CLI action, not a user-authorized model substitution, and violated the task's Astra-only constraint. The sidecar reported it immediately, stopped the still-running owned CLI after checking its process identity, and preserved `auxiliary-model-stop.json`. Already-completed attempts remain recorded as nonconforming; successful root-thread output does not erase the violation.

The supported [CLI `/rename` command](https://learn.chatgpt.com/docs/cli/slash-commands#rename-the-current-chat-with-rename) assigns a literal saved chat name. The runner now does this before the first prompt and checks native `session_index.jsonl` for the exact name, failing before model input if naming is unconfirmed. The no-model check above proved that mutation, and the accepted pilot proved its actual effect: only Astra sampling. A response header named `x-codex-safety-buffering-faster-model` advertises Luna; that header alone is not a sampling request. The oracle distinguishes it from native sampling diagnostics and includes ephemeral threads that the main rollout misses.

### Task 2.1 acceptance

Accepted thread `01a080da-3370-7e73-baa3-52c08fa6ce31`, one native TUI process and one model turn `01a080da-4adc-70d0-8629-435894f0a5a9`:

| Layer | Independent evidence |
| --- | --- |
| Parser | Native `features list` exited 0 and lists `context_management ... true`. |
| Effective setting | Native `config/read` returned `features.context_management.experimental_mode=true` in the same owned home. |
| Eligibility | Native `account/read`: ChatGPT Pro; real Astra turn succeeded on the pre-existing subscription route. |
| Runtime activation | Native `feedback_tags` emitted from `try_run_sampling_request`, model Astra, contains `ContextManagement`. Unlike config echo, this records the active sampling path. |
| Automatic continuation | PreCompact `auto` 11:50:04.179Z; native compacted record 11:50:22.475Z; PostCompact `auto` 11:50:23.017Z; SessionStart(`compact`) receipt 11:50:23.711Z. All belong to the same turn. |
| Immediate delivery | Native developer-context row at 11:50:23.856Z contains the new SessionStart marker. Native sampling log id 2028 follows at 11:50:23.868Z with `ContextManagement` still enabled. |
| Earlier invariant after continuation | Final 11:50:30.159Z chooses `winning-allowed` (blue, 10008), excludes red despite its score 99999, and echoes `SessionStart_compact_6d9b25ab8f5a`. Native task completes at 11:50:30.185Z without another user turn. |
| Route and termination | Native sampling models exactly `[gpt-6-astra]`; root provider `openai`, ChatGPT auth, existing `127.0.0.1:10100/v1`; native and worker natural exit 0. |

`acceptance.json` was generated with:

```powershell
./.venv/Scripts/python.exe -X utf8 tests/native-context-contracts-oracle.py "$env:TEMP/native-context-contracts-615a7db9a5a14869859acda2ad0f02f8" --check-contract pilot
```

Exit 0, no contract failures. This proves the bounded context pilot and survival of its early rule; it does not claim history-search tooling, a quality advantage over ordinary compaction, global activation, or task 2.2 completion.

### Task 1.1 acceptance

Accepted thread `01a080db-f2e8-7933-9d76-f4a08dfa1c2e`. Native PID **17768** remained alive through creation, update, manual compaction and automatic continuation; PID **31868** then resumed that same saved thread. Native `hooks/list` reported all five owned hooks trusted after ordinary CLI review. The model did not receive the skill path or revision token in the user prompts: supported hook JSON supplied the path; actual file tools returned the procedure; the independent oracle checked the opaque revision and numerical application.

| Requirement | Actual UTC ordering and independent result |
| --- | --- |
| Startup additionalContext | SessionStart(`startup`) 11:51:42.722; native developer row 11:51:42.849; first final echoes `SessionStart_startup_d58f5e698ea0`. |
| Newly created skill, current turn | PostToolUse after the seed read creates revision 1 at 11:51:50.139 and delivers its path in developer context at 11:51:50.252. Native file read 11:51:55.652, successful result 11:51:56.967; final 11:52:01.620 returns **58** and `REV1_9f51edcfa907`. No restart or intervening user prompt. |
| Updated skill, same CLI | Owned runner writes revision 2 at 11:52:02.663 while PID 17768 is alive. UserPromptSubmit delivers the current path at 11:52:04.278. Native read 11:52:10.627/result 11:52:12.256; final 11:52:16.949 returns **90** and `REV2_6c6f6114f361`. |
| Manual compaction | Native `/compact` request 11:52:17.887; PreCompact(`manual`) 11:52:18.640; native compacted 11:52:42.521; PostCompact 11:52:43.048; SessionStart(`compact`) 11:52:44.177, developer row 11:52:44.293. Subsequent actual read result 11:52:51.969; final 11:52:57.628 returns 90, revision 2 and `SessionStart_compact_c915a9b1ccdb`. |
| Actual automatic mid-turn compaction | Turn `01a080dd-42a9-7362-bea3-fed0d6b6b2e1` starts 11:52:59.317. Separate candidate/late-step reads finish; PreCompact(`auto`) 11:53:12.949; native compacted 11:53:46.415; PostCompact 11:53:47.058. No new turn starts until this turn ends 11:54:00.654. |
| Immediate automatic delivery and use | SessionStart(`compact`) 11:53:47.826; native developer row 11:53:47.935 precedes immediate Astra sampling log **2782**, 11:53:47.949. Current skill read 11:53:52.729/result 11:53:54.081. Final 11:54:00.597 returns **winning-allowed**, **90**, revision 2 and `SessionStart_compact_01c548da1512`. |
| Resume | Original native process exits 11:54:03.278; PID 31868 starts with the same thread id. SessionStart(`resume`) 11:54:10.957; developer context 11:54:11.076. Actual read result 11:54:18.626; final 11:54:23.232 returns 90, revision 2 and `SessionStart_resume_fe727474d26d`. |
| Other selected additionalContext events | Each of the five final answers echoes its distinct UserPromptSubmit receipt and latest PostToolUse receipt. Native developer rows independently contain those markers before the corresponding final. PreCompact/PostCompact record lifecycle only. |

Supported delivery established here is **hook additionalContext naming the current skill file, followed by an actual read and application in the already-running CLI**. This does not establish automatic refresh of the initial `/skills` inventory, watcher latency, or atomic package publication. Those mechanisms are not required to claim this demonstrated delivery path; publication/failure isolation and the rest of autonomous evolution remain separate tasks.

### Final verification and handoff

The strengthened oracle correlates native compacted records with PreCompact/PostCompact, one native PID covering the whole turn, no intervening turn, developer-context receipt before the immediate next sampling request, and the correct final within that turn. It also checks successful native skill-result content for both revisions, natural exits, subscription identity and auxiliary sampling. Full source/fixture review supplements these finite checks; this is not an adversarial evaluator-isolation claim.

Commands for the accepted native cases:

```powershell
./tests/native-context-contracts.ps1 -RunModelProbes -Scenario pilot -Transport tui -AutoCompactTokens 8000
./tests/native-context-contracts.ps1 -RunModelProbes -Scenario skills -Transport tui -AutoCompactTokens 8000
./.venv/Scripts/python.exe -X utf8 tests/native-context-contracts-oracle.py "$env:TEMP/native-context-contracts-615a7db9a5a14869859acda2ad0f02f8" --check-contract pilot
./.venv/Scripts/python.exe -X utf8 tests/native-context-contracts-oracle.py "$env:TEMP/native-context-contracts-8b9b787f78484c8488d5f290e547e127" --check-contract skills
```

Both acceptance commands exit 0 with no contract failures; `acceptance-ordered.json` preserves the final stricter result beside each original `acceptance.json`. The same oracle exits 1 for the preserved natural-exit-0 `undetermined` attempt and for the earlier non-Astra auxiliary-title attempt (`negative-contract-ordered.json`). No further model calls were needed for these checks.

Final local verification: all three owned PowerShell scripts parse without errors; Python AST parses; no-opt-in runner returns SKIP; six relative document links resolve; both changes pass `openspec validate CHANGE --strict`. Independent CSV evaluation checks all 902 candidate rows, winner/excluded counterexample, unchanged seed and late-step inputs, and the exact final revision-2 file. Each accepted run's auth copy is absent. The retained 19 attempt/cache/reduction directories contain no remaining `home/auth.json` copy. These checks and inventory are in the original static cache as `resumed-verification.json`, `resumed-fixture-verification.json` and `resumed-attempt-inventory.json`; `resumed-final-source/` and its hash manifest preserve the final owned sources without replacing earlier snapshots.

The no-model shell comparison is reproducible with a fresh root (the two arms share only that new parent directory):

```powershell
$comparisonRoot = Join-Path $env:TEMP ('native-context-contracts-shell-' + [guid]::NewGuid().ToString('N'))
./tests/native-context-contracts-shell.ps1 -EvidenceRoot $comparisonRoot -Arm baseline
./tests/native-context-contracts-shell.ps1 -EvidenceRoot $comparisonRoot -Arm filtered
```

Next concrete step belongs to main: integrate the verified child-only shell selection where its native consumers need it, then continue the remaining implementation and global lifecycle tasks using these receipts. Only autonomous **1.1** and adopt **2.1** are closed by this evidence. Global hooks remain intentionally suspended; this probe supplies no authorization or requirement to restore them. No runtime requirement remains unsupported within these two bounded tasks; archive and broader delivery remain main's responsibility.
