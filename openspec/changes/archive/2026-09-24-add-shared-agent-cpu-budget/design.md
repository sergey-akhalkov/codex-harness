## Context

See [proposal.md](proposal.md) for motivation and [the CPU requirements](specs/shared-agent-cpu-budget/spec.md) for acceptance. The owner chose an aggregate 75% ceiling across local agent work, explicit uncapped invocation, and automatic launch with a warning when CPU admission fails. These are separate modes; a warned failure is not successful enforcement.

Current source provides useful foundations:

| Owner | Inspected behavior and implication |
| --- | --- |
| `crates/harness-core/src/process.rs` | `Limits` already supports CPU percentages. Windows process creation uses a job-list attribute and suspended creation. Ordinary jobs have kill-on-close containment; `new_named` deliberately rejects an existing name. Reuse creation and verification primitives without weakening those ownership rules. |
| `crates/harness-core/src/native_launcher.rs` | The ordinary Windows session path creates a per-session job with default limits. A task-control path returns through a separate runtime, and shared services start as siblings. Changing this one job's percentage would neither cover every route nor produce a common budget. |
| `crates/harness-core/src/heavy_command.rs` and `crates/codex-harness/src/heavy_command_cli.rs` | An existing account-wide queue admits one batch tree, with a default 50% CPU budget, memory and deadlines. Nested calls verify membership and avoid double CPU throttling. Arbitrary commands and interactive sessions are outside that queue. See [the native guide](../../../../docs/rust-native.md#heavy-command-budget). |
| `crates/harness-core/src/task_runtime.rs`, `process_service.rs`, `process_service_wmi.rs` and `broker_launch.rs` | Existing services have independent lifecycle and launch paths. WMI process creation is used by the service infrastructure; ordinary parent-job inheritance is therefore not a complete admission mechanism. |
| `openspec/specs/linked-global-kit/spec.md` | Upstream Codex must remain launchable without a working shared checkout. The CPU policy must coexist with this recovery contract. |

Discovery established source behavior, not working aggregate enforcement. No new limit was installed and no load fixture was run for this proposal. Reported overload is motivation to measure the actual workload; it does not establish whether a particular kernel quota, inheritance path or measurement method is responsible. Private incident inputs and process identities remain outside this repository.

## Goals / Non-Goals

**Goals:** Use one host-account resource owner, preserve concurrent sessions and existing process cleanup, integrate through installed entry points, and make missing enforcement obvious without preventing launch.

**Non-Goals:** A whole-PC cap, a security sandbox against deliberate same-account evasion, global throttling of unrelated applications, model/provider changes, a shared model server, new memory limits, or a general process scheduler. Do not replace existing deadline, cancellation or lifecycle owners with the CPU accounting group. GPU, memory pressure and storage contention require their existing owners; the CPU ceiling alone is not a promise that every source of desktop latency disappears.

## Decisions

### 1. One shared CPU-only Windows job above independent lifecycle jobs

Use one account-wide CPU accounting job with hard-cap rate 7500 and independently owned child jobs for sessions, batch commands and shared services. Establish the outer-to-inner hierarchy before resuming newly created payloads. The common job must not acquire session kill-on-close or per-batch memory/deadline behavior. Closing one participant must not kill every agent or lift the cap for remaining participants.

Extend the existing process abstraction with a dedicated shared-budget lifetime and creation path. Preserve `Job::new_named`'s collision refusal for exclusive lifecycle jobs. Shared creation/reconnection uses the existing account-local ownership and locking facilities, verifies the object owner and effective settings, and protects against simultaneous first launches creating separate groups. Scope identity to the Windows account, not a project, `CODEX_HOME`, terminal tab or build directory. Retain enough live handles to rejoin the same job while members remain; a replacement manager must not create a parallel allowance.

Microsoft documents that nested jobs can group peer process trees and that assignment order matters. Their accounting includes nested members, while termination of a parent reaches its descendants. This is why CPU accounting and cleanup need distinct ownership. See [Nested Jobs](https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs).

Separate 75% jobs were rejected because aggregate use increases with concurrency. Reusing the exclusive batch queue for entire sessions was rejected because a long-lived conversation would block other agents. Affinity alone trades away cores without expressing the requested shared duty budget. Periodic process discovery/suspension has a startup race and introduces a scheduler and recovery burden. No such watcher is needed for the normal admission path.

### 2. Admit through every installed launch owner

Wire the common admission into the checkout-independent Codex bootstrap, the manager's task/executor/resume path, and shared tool-service startup. Keep native argument, environment, working-directory, streams and exit-code forwarding. Do not route arbitrary tool commands through PowerShell to enforce CPU policy. Root admission and ordinary inheritance cover direct native children; service-owned sibling launches require their own admission.

Use the existing `process.rs` creation machinery rather than starting a payload and then attempting to catch it. Microsoft exposes `PROC_THREAD_ATTRIBUTE_JOB_LIST` for job assignment during process creation; validate the complete ordered list through the existing native entry point. See [UpdateProcThreadAttribute](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute).

Installation must exercise Codex, terminal dispatch and MCP routes outside the checkout. Resolve installed upstream executables with the existing identity/update rules; preserve original executable access and avoid recursive launcher discovery. New supported agent integrations must enter the same policy rather than gain a new budget. Checking process names alone or limiting Windows Terminal itself is insufficient.

### 3. Compose with the existing heavy-command policy

Keep the current batch queue, memory limit, deadlines, exit codes and cleanup. A batch command launched inside the common group reuses the outer CPU limit after kernel membership verification. Native builds launched outside a conversation must join the same common group as well. The old automatic per-batch CPU default must not silently remain as a second multiplicative default.

Preserve deliberately configured lower per-operation limits and report their effective host-relative value. Windows expresses an inner rate relative to the rate-controlled parent. Translate intentional limits against the verified parent rate and round conservatively, rather than treating an inner percentage as another percentage of the whole machine. Existing machine-local heavy-policy files must be classified explicitly during migration: distinguish an intentional override from an old default, preserve the former, and report ambiguous legacy state instead of silently changing it. A project wrapper that independently installs another cap can still constrain its own command; do not remove consumer-owned limits automatically or claim its old limit disappeared.

The platform rule and hard-cap semantics are documented in [JOBOBJECT_CPU_RATE_CONTROL_INFORMATION](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_cpu_rate_control_information). Readback proves configuration only; the installed workload measurements below remain required.

### 4. Explicit exceptions start outside the common CPU ancestry

Provide a per-invocation CPU-mode selector and an installed status operation. Exact option spelling belongs to the existing CLI conventions, but the modes and scope are normative. An exception from an already capped session cannot be implemented by clearing an environment variable: its children inherit the capped ancestry. Use an existing account-owned independent launch service or the bootstrap's outside-group control path to create that one uncapped invocation. Verify this path with a caller that is itself capped.

Reuse existing service transport, executable identity checks and console/stream attachment; do not introduce a second general service framework. The small admission/control process must perform only bounded launch and status work, not host agent payloads outside the budget. Normal payloads created through it join the group before execution; exceptions are explicit. Shared MCP services stay capped even when called by an uncapped client. No new provider/model conversation is required to test this mechanism.

Ordinary descendants must not be granted silent breakaway merely to make exceptions convenient. Uncapped execution can exceed the normal all-agent ceiling; make that consequence visible at launch and in status. A copied marker does not authorize changing the common job.

### 5. Preserve availability on CPU-policy failure

The owner selected fail-open behavior. Attempt default admission before payload execution; if setup, reconnection, configuration or membership verification fails, warn and launch the original requested payload once with native semantics preserved. The warning identifies the requested ceiling, failed stage/cause, affected scope and recovery action. It must say enforcement is unverified when that is all the implementation knows; it must not assert that inherited limits were removed.

Keep this minimal path in the independently installed bootstrap. Optional checkout/configuration breakage must not disable either upstream access or the attempt to establish the budget. Do not make a successful update, model request, network service, dependency installation or compilation a startup prerequisite. After a payload starts, any later failure changes reported coverage; it never triggers automatic replay. Restore partial pre-start resources safely before fallback and preserve other sessions' handles and settings.

This means the default is intentionally best effort on failure, not an unconditional machine-wide guarantee. Status distinguishes `capped`, explicit uncapped operation and degraded/unknown enforcement. Reconcile the existing decision and installation guides with the linked-global-kit delta when implementing.

### 6. Validate consumption and useful interaction through actual routes

Start with a small Rust CPU fixture through one installed agent-owned process route. Observe job membership, effective rate and normalized process CPU time, then exercise two simultaneous independent session trees plus a shared backend. Use the actual machine logical-processor count, not a runtime count reduced by quotas. For a declared interval, aggregate CPU percentage is `100 * CPU_seconds / (elapsed_seconds * host_logical_processors)`; prevent double counting nested jobs or descendants that are already included in a parent accounting total.

Retain a bounded longer representative build/test workload as well as the short saturation fixture. Compare kernel rate, ancestry, process CPU time and the host's displayed utilization if they disagree; scheduler-cycle and display counters must not be treated as interchangeable without verification. Set the observation duration and allowable sampling error before judging results. A persistent unexplained overshoot blocks enforcement acceptance, even if native readback says 7500. Do not substitute fewer test threads, weaker assertions or longer application deadlines for a working limit.

Exercise normal interactive input/cancellation while the group is loaded, identify latency observations and remaining CPU-external limits, and verify session exit, shared-service continuation, malformed policy, object collision, degraded fallback, explicit exceptions, update and rollback. Reuse native launcher/process/service/heavy-command tests and existing private test state. Fixtures are controller-free, use synthetic inputs and make no provider calls; they do not need access to a consumer's live workload or external equipment. Keep detailed runtime evidence in the established local verification lifecycle, not a new public report store.

## Risks / Trade-offs

- **Incomplete root coverage** -> Include both agent applications and shared service owners in installed acceptance; list old sessions requiring restart and degraded launches explicitly. Do not declare coverage complete based on launcher configuration alone.
- **Nested job and console constraints** -> Verify ordered assignment, terminal dispatch, task-control processes and explicit exceptions with native fixtures before global activation. Preserve existing containment rather than enabling general breakaway.
- **One participant affects all others** -> Keep aggregate CPU handles separate from termination handles and retain independent service/session shutdown behavior. Test crashes and concurrent reconnects.
- **Failure can consume more than 75%** -> This is the owner's explicit availability choice. Warn immediately, retain degraded coverage in status, and leave unaffected sessions capped.
- **Ordinary commands or tool requests take longer** -> Preserve cancellation and real application deadlines. Measure interactive responsiveness and distinguish throttled timing from unrestricted performance; do not claim that a 75% cap reserves the remaining capacity against unrelated applications.
- **Old lower caps compound throttling** -> Reconcile kit-owned defaults, retain intentional limits and report effective rates. Consumer-owned launchers need an explicit separate migration when their owners choose it.

## Migration Plan

1. Implement and verify the common CPU owner and one installed vertical path, then connect the remaining session and service launch owners. Use the repository's current capped verification entry point during development; this planning change has not changed the live budget.
2. Extend the existing installation/update preview and owned registration lifecycle for Codex, with machine-local policy and rollback. No global activation occurs merely because source artifacts exist.
3. Inspect existing sessions and services. Prefer a documented safe restart for an incompatible old job hierarchy; adopt live work only if complete membership and lifecycle verification is possible. Preserve unfinished work and show incomplete activation until all ordinary routes have transitioned.
4. Exercise installed entry points from a synthetic consumer outside this checkout and complete the acceptance matrix. Reconcile the native-command guide, installation guide and project decision record in their existing homes.
5. Roll back only owned registrations/artifacts while retaining running identities and later user edits. Report the resulting coverage and required restart without silently lifting a cap on unrelated active work.

Standards impact: local development-host resource control only; no consumer language, controller behavior or functional-safety claim changes. Existing installation ownership, local IPC access checks and publication boundaries remain applicable.
