## ADDED Requirements

### Requirement: Idempotent completion diagnostics

Stop and SubagentStop SHALL deliver an unchanged diagnostic outcome at most once per originating agent across native and fallback handlers. A successful clean or deleted result MUST NOT request continuation. New findings and unresolved checks SHALL remain explicit; repetition suppression MUST NOT mark an incomplete check clean or conceal a later changed input. A newly encountered failure at completion MAY request one continuation, respecting the native active-stop flag.

#### Scenario: Both completion handlers observe the same backend failure
- **WHEN** the native handler completes a failed Markdown check and the command companion receives the same unchanged input
- **THEN** the companion does not duplicate that check or feedback, and repeated Stop events do not block or print the same failure again

#### Scenario: Source changes after a native completion
- **WHEN** source bytes, project configuration or backend registry change before the companion checks completion
- **THEN** the new input requires reconciliation and is not covered by the previous completion

#### Scenario: Completion clears a previous error
- **WHEN** a Stop check establishes authoritative current empty diagnostics
- **THEN** clearance is reported without requesting continuation

### Requirement: Markdown links into sibling directories

Markdown diagnostics SHALL validate explicit local file and fragment links into sibling directories using bounded read-only dependency access. Workspace enumeration and source operations MUST remain scoped to their applicable roots. Unrelated local files, network resources and implicit expansion to an entire parent directory MUST NOT become accessible through this dependency allowance. Missing files and fragments SHALL remain language diagnostics rather than client protocol failures.

#### Scenario: Existing document links into a sibling checkout
- **WHEN** a Markdown document links to an existing file and heading in a sibling checkout
- **THEN** the real language server completes the check without an additional-workspace-root exception

#### Scenario: Linked target is missing or has changed
- **WHEN** a local or sibling link targets a missing file or heading, including after a linked heading changes
- **THEN** diagnostics report the actual link error and clear after correction

#### Scenario: Server requests an unrelated resource
- **WHEN** a Markdown client request targets an external resource absent from the document's explicit local dependencies
- **THEN** the client preserves its resource boundary without recursively scanning the surrounding filesystem
