## Purpose

Maintain a reusable skill library whose accepted changes improve verified work within declared context and learning budgets, without requiring the user to curate each change or accumulating unsupported instructions.

## ADDED Requirements

### Requirement: Evidence-driven knowledge routing

During authorized mutable project work, the agent SHALL consider verified non-obvious solutions, repeated friction, corrected failures and skill-use feedback as learning signals. It SHALL distinguish project facts, tentative observations, reusable procedures and executable automation, reuse the owning project record or tool where appropriate, and check existing skills before creating a candidate. Routine success, duplicated knowledge and unverified conclusions MUST NOT automatically create active skills. Explicit read-only or exploration-only task boundaries SHALL remain effective. This workflow SHALL NOT treat orchestration-incubator votes, account-window pacing or process succession as its intake or delivery mechanism. Process, orchestration, requirement, tool and unclear or material changes remain outside this workflow unless they yield a verified reusable procedure in owned skill scope. A kit-backlog item about owned skills MAY start an ordinary evolution episode; that eligibility SHALL NOT by itself write a skill package.

#### Scenario: Useful fact without a reusable procedure
- **WHEN** a task establishes a project-specific prerequisite for an existing test command
- **THEN** the agent updates or references the owning project record without inventing a new skill or duplicating the authoritative command

#### Scenario: Verified costly failure yields a procedure
- **WHEN** one investigated failure establishes a reusable diagnostic method with a checkable result
- **THEN** the agent can prepare a candidate with that evidence without requiring an arbitrary number of repeated incidents

#### Scenario: Routine or read-only work
- **WHEN** a task yields no new durable knowledge or explicitly forbids file changes
- **THEN** evolution performs no unnecessary writes and does not treat the global learning policy as permission to override the task boundary

#### Scenario: Orchestration friction is not a procedure
- **WHEN** an executor reports process or quota friction without a reusable owned procedure
- **THEN** evolution does not create a skill from that signal; the observation remains available to the orchestration feedback loop

#### Scenario: Kit backlog authorizes planning not publication
- **WHEN** a kit-backlog item concerns an owned skill
- **THEN** evolution may start a bounded episode from that authorized work and still requires skill-evaluation before activation

### Requirement: Autonomous bounded maintenance

The installed workflow SHALL create, update, consolidate and retire owned skills within the configured project and kit ownership boundaries without asking the user to review each ordinary change. Each candidate SHALL identify the current library, the proposed library change, its applicable tasks and expected observable benefit. Shortening, narrowing applicability, consolidation and retirement SHALL be eligible improvements alongside creation. Semantic changes SHALL pass the same skill-evaluation contract and context admission rules before activation. A failed, inconclusive or unavailable check SHALL retain the usable previous version and a concise candidate status; it MUST NOT cause repeated approval requests, unbounded improvement loops or unconditional extra model calls on every task. Candidates SHALL remain outside discovery until accepted; candidate count SHALL NOT be a success metric.

#### Scenario: Existing skill misses a case
- **WHEN** verified use exposes a missing case in an owned skill
- **THEN** the agent prepares and checks an update, activates it if accepted, and reports the outcome without requiring a manual authoring or approval step

#### Scenario: Evaluation cannot finish
- **WHEN** an evaluation times out or its required model or tool is unavailable
- **THEN** the candidate remains pending with the cause, the previous skill stays usable, and retries require new evidence or a changed condition

### Requirement: Project ownership and portable records

Project skills and durable supporting knowledge SHALL live in the owning project's Git working tree using its existing records and entry route. The workflow SHALL preserve unrelated changes and worktree boundaries, keep tentative evidence distinguishable from verified claims, and remain usable without personal session history or a separate memory service. Credentials, raw transcripts and machine runtime state MUST NOT enter tracked skill packages. Skill maintenance MUST NOT imply automatic staging, commit, push or edits to arbitrary other checkouts.

#### Scenario: Project already has memory records
- **WHEN** a project has an authoritative decision or verification record
- **THEN** the workflow reuses it and links relevant evidence instead of maintaining a competing copy

#### Scenario: Fresh clone and disconnected kit
- **WHEN** project changes have been committed through the normal authorized Git workflow and cloned without the original user's session state
- **THEN** project skills and their portable evidence remain readable and discoverable, including after the kit's registrations are disconnected

### Requirement: Candidate isolation and protected control rules

Unaccepted candidates and historical packages SHALL remain outside all effective skill discovery roots. The workflow SHALL distinguish owned skills from system, third-party, disabled and foreign skills; it MUST NOT overwrite, shadow or re-enable them without existing explicit authority. Candidate content and captured external instructions SHALL be treated as untrusted input. Evolution MUST NOT modify its controlling policy, evaluator acceptance rules, permissions, model routing or lifecycle enforcement to make its own proposal pass. Changes to those controls SHALL follow a separately authorized development change.

#### Scenario: Candidate attempts to loosen its evaluator
- **WHEN** a skill or its source evidence requests disabling a check, changing provider routing or modifying the evolution controller
- **THEN** that request does not alter the control rules and the attempted out-of-scope mutation prevents acceptance

#### Scenario: Existing foreign or disabled skill
- **WHEN** a candidate name or target collides with a third-party skill or an explicitly disabled skill
- **THEN** the foreign content and disablement remain intact and the candidate stays unactivated with a specific conflict status

### Requirement: Revision-safe publication and recovery

Publication SHALL bind evaluation evidence to the exact candidate package, baseline revision and applicable dependency context. It SHALL check current ownership and content before mutation, serialize competing writers for the same owned target, publish a coherent revision and preserve rollback material. A running consumer MUST NOT combine incompatible resource revisions from a partially published package. Interrupted publication SHALL either retain the prior usable revision or provide a recoverable explicit incomplete state without advertising a mixed revision as accepted. Recovery MUST NOT overwrite subsequent foreign edits.

#### Scenario: Two worktrees improve the same skill
- **WHEN** another writer changes the active source after a candidate's evaluation began
- **THEN** publication detects the baseline mismatch, preserves both parties' work and requires reconciliation with applicable revalidation before acceptance

#### Scenario: Failure during multi-file publication
- **WHEN** publication fails after one of several package resources changes
- **THEN** consumers cannot use a mixture as an accepted revision, recovery retains or restores the previous coherent package, and any remaining partial state is reported

#### Scenario: Foreign edit precedes rollback
- **WHEN** a file or registration has been changed by another owner since activation
- **THEN** rollback preserves that new content and reports the exact conflict instead of restoring a stale backup over it

### Requirement: Verified promotion to a shared source

A project-derived skill SHALL become global only after removing project-only assumptions and sensitive data, checking the declared dependencies and passing evaluation in a second independent project context as well as its origin. Accepted shared skills SHALL have one canonical kit source consumed through the installation lifecycle. Promotion SHALL reconcile overlapping local and global identities without deleting uncommitted project work or spreading facts to other repositories. Global registration SHALL be scoped to the accepted skill operation.

#### Scenario: Reusable method passes transfer checks
- **WHEN** a project candidate works in its origin and another project under its generalized contract
- **THEN** the workflow can publish it to the owned shared source and reconcile its registration autonomously without manually copying it into each project

#### Scenario: Candidate contains a local assumption
- **WHEN** a proposed common skill depends on an origin-only path, secret or unavailable service
- **THEN** global promotion is withheld while any valid local skill remains usable

### Requirement: Consolidation and retirement with retained behavior

The workflow SHALL consolidate overlapping skills only when the replacement preserves the applicable scenarios and selection behavior. It SHALL evaluate retirement of owned obsolete or harmful skills based on verified replacement, retired workflow, dependency invalidation or measured lack of benefit. An accepted retirement SHALL first remove the skill from effective discovery and future selection reversibly, preserve its package and necessary recovery evidence outside discovery, and satisfy current-session awareness. Physical deletion SHALL be a separate ownership- and consumer-checked operation. Supported references and known consumers SHALL be checked before changing their supported behavior; unresolved dynamic consumer scope SHALL prevent automatic destructive removal. Lack of recent use, elapsed time, absence of a statistically detected difference or catalogue pressure alone SHALL NOT prove obsolescence or authorize disabling a required capability. Rare but important scenarios SHALL remain protected by their applicable acceptance checks.

#### Scenario: Two overlapping skills are consolidated
- **WHEN** a replacement passes both original behavior suites and coexistence checks
- **THEN** activation updates the applicable references and catalogue without leaving competing active identities, while the previous packages remain recoverable outside discovery

#### Scenario: Infrequently used skill has unknown consumers
- **WHEN** maintenance observes low usage but cannot establish that the skill is obsolete or safely replaced
- **THEN** it retains the skill and records the uncertainty instead of deleting it

#### Scenario: Removing a redundant skill improves the library
- **WHEN** a library-without-skill comparison satisfies unchanged protected scenarios and confirms its declared benefit
- **THEN** the workflow retires only the owned skill reversibly, reconciles its references and session visibility, and retains a recoverable prior revision outside discovery

#### Scenario: Retirement evidence is inconclusive
- **WHEN** both variants pass a small sample but the declared comparison cannot establish the removal claim
- **THEN** the active library remains unchanged and the removal candidate stays inactive with an inconclusive result

### Requirement: Event-driven library review

Maintenance SHALL consume relevant signals from authorized active work: observed failures, inappropriate selection, repeated retrieval or execution overhead, overlap with an accepted replacement, workflow or dependency retirement, changed model/runtime assumptions, catalogue admission pressure, and a user-requested usage-analysis report. It SHALL consider amendment or removal as well as addition, deduplicate an unchanged signal for the same applicable revisions, and select bounded reviews using expected benefit, concrete risk and the remaining learning budget. Review SHALL NOT require a model call after every task or background scans of inactive repositories. Ordinary observations SHALL distinguish eligible task opportunities, actual selection and demonstrated application; no eligible observations SHALL remain unknown usefulness, not zero usefulness. Changed assumptions SHALL invalidate affected evidence without silently deleting the skill or claiming previous measurements remain current.

#### Scenario: A description repeatedly attracts unrelated requests
- **WHEN** observed use shows inappropriate selection under the same skill revision
- **THEN** maintenance considers narrowing, consolidation or retirement against the current library and includes those unrelated requests in the comparison

#### Scenario: A model update changes the evaluation context
- **WHEN** a model or runtime change invalidates a skill's recorded benefit assumptions
- **THEN** the workflow marks that benefit as needing reassessment and schedules only a justified bounded review; it does not claim automatic continued savings or disable the skill solely because evidence is old

#### Scenario: An unchanged failed idea is submitted again
- **WHEN** a new name or another task repeats the same failed candidate without new relevant evidence
- **THEN** the existing decision and expenditure are reused instead of creating another model-backed evaluation episode

### Requirement: Usage analysis in the library lifecycle

The installed kit SHALL expose `skills-usage-analysis` and `codex-harness skills usage` as the inspectable usage surface of this lifecycle. Evolution SHALL treat a completed analysis report as a bounded review signal and MAY prepare disable or retirement candidates from it. Listing a candidate or reporting last invocation MUST NOT publish a retirement. User-confirmed disable from that surface SHALL be reversible discovery removal for owned non-protected skills and SHALL NOT skip consumer-checked physical deletion.

#### Scenario: User asks for library hygiene
- **WHEN** the user requests usage analysis during authorized work
- **THEN** the agent uses `skills-usage-analysis`, which runs the usage command and proposes candidates without applying them
- **AND** a later confirmed disable still preserves a recoverable package outside discovery

### Requirement: Inspectable accepted revision identity

After an accepted creation, update, consolidation or retirement is published, the workflow SHALL record a compact identity for that operation: skill name, canonical path, revision and operation kind. Authorized consumers SHALL be able to detect that the library changed without reading skill bodies or conversation transcripts. This identity SHALL NOT constitute in-session delivery, orchestrated process succession, or a second event bus.

#### Scenario: An orchestration consumer needs the new revision
- **WHEN** a skill revision is accepted and published
- **THEN** the compact identity is available so a separate succession path can decide whether an orchestrated worker must refresh
- **AND** that succession, if used, does not close this workflow's same-session compact requirement
