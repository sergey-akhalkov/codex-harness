## Purpose

Automatically give Codex current language diagnostics after source edits so the agent can react without a separate user request, while preserving edits, workspace isolation and honest failure reporting.

## ADDED Requirements

### Requirement: Automatic diagnostics across edit entry points

For supported source files, the integration SHALL automatically detect successful filesystem changes made by Codex native patch operations, completed shell commands and MCP edits, then request diagnostics without an explicit user or model diagnostic call. Detection SHALL include created, modified, renamed and deleted files, tracked and untracked files, pre-existing dirty files and multi-file operations. A command that writes files and subsequently fails SHALL still trigger diagnostics for its actual changes. A read-only or no-change operation MUST NOT cause a full-project diagnostic run.

#### Scenario: A native patch introduces an error
- **WHEN** Codex patches a supported source file to contain a known language error
- **THEN** automatic diagnostics identify the file, position and error without the user asking for a check

#### Scenario: Shell changes are not represented by a simple patch
- **WHEN** a completed shell command updates a pre-dirty file and creates an untracked source file, including when the command exits nonzero after writing
- **THEN** both current contents are checked without relying only on command-text parsing or the tracked Git diff

#### Scenario: An MCP rename updates several files
- **WHEN** an MCP edit changes several files or moves a source file
- **THEN** the changed and affected source identities are reconciled, deleted-file diagnostics are cleared, and the surviving files receive current diagnostics

#### Scenario: A read-only tool completes
- **WHEN** the tool made no filesystem change relevant to language analysis
- **THEN** no unnecessary full-project analysis or package installation is triggered

### Requirement: Agent-visible bounded delivery

Diagnostic summaries SHALL reach the originating Codex turn through a supported model-visible integration point before it can report completion of the edited work. Synchronous diagnostic waits SHALL have a documented finite timeout, initially at most 30 seconds per completed edit batch; cold startup SHALL return an explicit pending or unavailable status within that bound. The integration SHALL automatically deliver or reconcile a pending result before a later completion claim. It MUST NOT depend solely on logs, UI notifications, a prompt asking the model to run diagnostics, or an asynchronous result that can arrive after completion. The original edit result and exit status SHALL remain intact.

#### Scenario: A file edit completes and the server responds
- **WHEN** the automatic check completes within its budget
- **THEN** the originating agent receives the diagnostic summary together with the tool interaction before its next completion response

#### Scenario: A server is still initializing
- **WHEN** the initial diagnostic request cannot finish within the finite budget
- **THEN** the agent receives an explicit pending/unavailable result, the turn does not stall indefinitely, and the unresolved check is reconciled automatically or remains explicitly unresolved at completion

#### Scenario: The first edit precedes backend readiness
- **WHEN** a session edits a file before its diagnostic MCP is ready and immediately attempts to finish
- **THEN** the pre-edit state or equivalent evidence preserves that edit for reconciliation, later backend startup cannot absorb it as an unchanged baseline, and completion receives current diagnostics or an explicit unresolved result

#### Scenario: A subagent completes with pending diagnostics
- **WHEN** a subagent attempts to return after editing while its diagnostic work is still pending
- **THEN** that child's applicable workspace and invocation are reconciled before its return, and its result contains current diagnostics or an explicit unresolved status without substituting the parent or another child's results

### Requirement: Current revisions and complete result semantics

Every result SHALL identify its project, file, analyzed content revision or equivalent generation, server and status. Diagnostics SHALL retain severity, range, message and code/source when provided. Results SHALL distinguish clean, diagnostics present, pending, unavailable, failed and stale. Missing results, timeouts and empty results that cannot be associated with completed analysis of the current contents MUST NOT be treated as proof of no errors. An authoritative current empty diagnostic set SHALL be accepted as successful clearance, including a current push notification containing an empty diagnostics array. New edits SHALL supersede old requests; results for obsolete contents MUST NOT replace current diagnostics. Correcting an error SHALL clear it. Bounded output SHALL disclose omitted diagnostic counts and make the complete result retrievable.

#### Scenario: A later edit completes before an earlier request
- **WHEN** diagnostics for the old contents arrive after a correction
- **THEN** the old result is discarded or labeled stale and cannot reintroduce the corrected error as current

#### Scenario: A known error is corrected
- **WHEN** the server completes analysis of the corrected revision
- **THEN** the agent sees that the previous diagnostic cleared, with an explicit successful check of the new revision

#### Scenario: The report exceeds the output budget
- **WHEN** a check produces more diagnostics than fit in its summary
- **THEN** the summary preserves error visibility, reports truncation and provides a way to retrieve the complete scoped result

### Requirement: Project inputs and dependent diagnostics

Analysis SHALL use the project's existing language version, compiler options, dependency resolution, roots and encodings. The integration SHALL process dependency-affected diagnostics exposed by the language server after a source or configuration change. It MUST NOT rewrite compiler settings, project sources or lockfiles merely to obtain a clean check. Unsupported files, missing dependencies and invalid project inputs SHALL produce actionable capability status rather than false language errors or a false clean result.

#### Scenario: A changed declaration breaks an importing file
- **WHEN** the backend reports an error in a dependent file following the edit
- **THEN** the automatic result includes that relevant error with the correct file identity

#### Scenario: A project configuration changes
- **WHEN** compiler options, an include path or a language configuration file changes through a covered edit route
- **THEN** subsequent diagnostics reflect the updated project inputs and invalidate results derived from the previous configuration

### Requirement: Isolation and non-destructive failure behavior

Automatic diagnostics SHALL execute within the session's applicable roots and permissions and SHALL never modify source files or execute automatic fixes. They SHALL isolate concurrent workspaces and distinguish concurrent tool invocations, including subagents sharing a parent session identifier. Failure, timeout, recursion or backend shutdown MUST NOT undo a successful edit, conceal its result, kill another consumer or create an unbounded retry loop. Analysis requests generated by diagnostics MUST NOT recursively trigger the same check.

#### Scenario: Two sessions edit concurrently
- **WHEN** diagnostic requests overlap across projects or same-project worktrees
- **THEN** each agent receives results for its own applicable root and current contents without cross-session substitution

#### Scenario: The diagnostics backend crashes after a successful patch
- **WHEN** analysis fails
- **THEN** the edit stays intact, the original patch result remains visible, and the agent receives an explicit diagnostic failure with a bounded recovery path

### Requirement: Verification through real Codex edits

Acceptance SHALL prove automatic detection and model-visible delivery through actual Codex patch, shell and MCP edit entry points. Rust, TypeScript, JavaScript, PowerShell, Python, Delphi, C++, C#, JSON, Markdown, TOML, XML, CMake and Bash SHALL each have a known-error/correction scenario on representative source. No other language is required by this change. The suite SHALL cover stale responses, multi-file changes, pre-dirty/untracked files, partial command failure, missing server, bounded timeout, dependent errors and concurrent roots. Direct calls to the diagnostic helper or mocked events alone MUST NOT establish automatic integration.

#### Scenario: Automatic diagnostics are declared delivered
- **WHEN** the feature is marked verified
- **THEN** the evidence shows actual edits, automatic checks and agent-visible error/clearance results, with no explicit request to invoke a diagnostic tool
