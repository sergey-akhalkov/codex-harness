## Purpose

Turn verified task experience into maintained reusable agent skills across repositories, without requiring the user to curate each change or accumulating unsupported instructions.

## ADDED Requirements

### Requirement: Evidence-driven knowledge routing

During authorized mutable project work, the agent SHALL consider verified non-obvious solutions, repeated friction, corrected failures and skill-use feedback as learning signals. It SHALL distinguish project facts, tentative observations, reusable procedures and executable automation, reuse the owning project record or tool where appropriate, and check existing skills before creating a candidate. Routine success, duplicated knowledge and unverified conclusions MUST NOT automatically create active skills. Explicit read-only or exploration-only task boundaries SHALL remain effective.

#### Scenario: Useful fact without a reusable procedure
- **WHEN** a task establishes a project-specific prerequisite for an existing test command
- **THEN** the agent updates or references the owning project record without inventing a new skill or duplicating the authoritative command

#### Scenario: Verified costly failure yields a procedure
- **WHEN** one investigated failure establishes a reusable diagnostic method with a checkable result
- **THEN** the agent can prepare a candidate with that evidence without requiring an arbitrary number of repeated incidents

#### Scenario: Routine or read-only work
- **WHEN** a task yields no new durable knowledge or explicitly forbids file changes
- **THEN** evolution performs no unnecessary writes and does not treat the global learning policy as permission to override the task boundary

### Requirement: Autonomous bounded maintenance

The installed workflow SHALL create, update, consolidate and retire owned skills within the configured project and kit ownership boundaries without asking the user to review each ordinary change. Changes SHALL have a stated purpose and expected observable effect. Semantic changes SHALL pass the skill-evaluation contract before activation. A failed, inconclusive or unavailable check SHALL retain the usable previous version and a concise candidate status; it MUST NOT cause repeated approval requests, unbounded improvement loops or unconditional extra model calls on every task.

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

The workflow SHALL consolidate overlapping skills only when the replacement preserves the applicable scenarios and selection behavior. It SHALL retire owned obsolete or harmful skills based on verified replacement, retired workflow, dependency invalidation or measured lack of benefit, preserving recovery and traceability. Lack of recent use alone SHALL not prove obsolescence. Supported static references and known consumers SHALL be checked; unresolved dynamic consumer scope SHALL prevent automatic destructive removal.

#### Scenario: Two overlapping skills are consolidated
- **WHEN** a replacement passes both original behavior suites and coexistence checks
- **THEN** activation updates the applicable references and catalogue without leaving competing active identities, while the previous packages remain recoverable outside discovery

#### Scenario: Infrequently used skill has unknown consumers
- **WHEN** maintenance observes low usage but cannot establish that the skill is obsolete or safely replaced
- **THEN** it retains the skill and records the uncertainty instead of deleting it
