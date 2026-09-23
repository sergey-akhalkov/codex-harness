## Context

See proposal.md. Baseline source is `2e8b309`. Installed CLI 0.155.1 and core connection pass the native read-only check. Two configured `ds`/`max` pool sessions independently inspected source. Their retained board findings identify unconditional WindowsApps PATH removal in executor dispatch/succession, no native session/result fields in receipts, no producer for the documented result artifact, and no executor watch command. The shell diagnostic actually observed Windows PowerShell 5.1 after the only installed PowerShell 7 disappeared from child PATH. Both diagnostic checkouts remain clean.

The build owner currently uses an 8 GiB / 50% CPU Job and a per-build-state exclusive lock; documentation still says 2 GiB. Existing resource admission supplies OS-backed leases and cancellation, but inspected call sites cover brokers/indexing, not arbitrary executor checks. These facts do not establish an existing general heavy-command queue.

## Goals / Non-Goals

Restore the usable installed path first, then make its normal completion loop observable. Keep native profiles, worktrees, leases, process Jobs, rollout reading, board records and release as the owners. Do not create a scheduler service, replace Codex tools, change providers or impose a new project-count/lifetime limit.

## Decisions

1. Shell bootstrap stays with the lead until executors have a permitted shell. Native CLI 0.155.1 rejects profile selection for app-server and its exported `ConfigReadParams` has no profile field. Use its supported model-free `debug prompt-input` runtime path, accepting only a single native developer permission block; user/AGENTS text cannot supply that block. This exercises actual profile/project precedence instead of reconstructing configuration. Preserve the owner's PATH for verified unsandboxed execution; retain the packaged-shell restriction for sandboxed execution and fail before model work if no permitted PowerShell 7 exists. Verify the selected executable's version. Packaged PowerShell rejects the Job-list process-creation attribute, so its bounded version-only probe uses ordinary process creation without running a profile or command. Carry the resulting child environment through the receipt because Windows Terminal can retain an older environment. No permissions override or additional installation is needed; succession's diagnostic uses its already-authorized recorded sandbox setting before stopping its predecessor.
2. Extend existing receipts with observed lifecycle and native identity; distinguish dispatch from native start. Reuse bounded native rollout reading for completion/error evidence, never inspect opaque reasoning state. A single executor observation command returns a compact result/event and exact continuation reference. Preserve explicit lead acceptance before destructive release. A completion receipt is evidence of execution state, not proof that claimed checks passed.
3. Extend existing resource admission and process Jobs for heavy-command execution. Reuse the installed budget rather than adding per-worker caps; keep machine values outside shared source. Determine the account-local setting and wire the actual native check consumer before claiming aggregate enforcement. An external command bypassing this entry point is outside its enforced scope and must be disclosed.
4. Extend the assignment renderer and team-lead operating contract once runtime capabilities work. Keep the schema compatible where optional fields suffice. Instructions point to tools for mechanical observation and resource admission; they do not ask the lead to search logs or manage queue grants. Explicit profile selection remains authoritative. No effort change is promoted without comparable evidence.

## Risks / Trade-offs

- Native diagnostic wording can change by CLI version: accept only an unambiguous permission block, surface failures and retain private evidence; never assume unsandboxed execution. The opt-in installed preflight check detects compatibility changes without model requests.
- A terminal environment can be stale: propagate the selected environment in the owned dispatch receipt and validate actual executor tool execution after delivery.
- A stopped host can leave partial source: continuation and release keep existing ownership checks; no automatic reset based on process exit alone.
- Quantitative baseline telemetry may be incomplete: use existing rollout accounting and report missing measurements. No subscription-saving inference from output size.

## Migration Plan

Run focused native regressions, commit the reviewed bootstrap snapshot and deploy through the normal immutable lifecycle. Resume the original executor conversations to prove permitted tools, then dispatch disjoint implementation outcomes from the new committed base. Complete installed synthetic acceptance and source hygiene before closing tasks. Retain prior immutable builds and normal recovery; preserve unrelated consumers if activation refuses.
