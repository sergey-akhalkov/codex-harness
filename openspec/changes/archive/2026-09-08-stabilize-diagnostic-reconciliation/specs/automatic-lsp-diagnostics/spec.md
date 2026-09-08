## ADDED Requirements

### Requirement: Bounded discovery with explicit coverage gaps

Change discovery SHALL be independent of language-analysis size and availability limits. Large unchanged files SHALL NOT invalidate a baseline merely because their size exceeds a language input limit. Changed files that cannot be analyzed SHALL remain explicitly unverified. Partial discovery SHALL preserve known observations without inferring absent files were deleted. Work SHALL remain bounded across directory traversal, file reads and journal lock waits.

#### Scenario: Large archived input is unchanged
- **WHEN** a repository contains an unchanged JSON larger than 8 MiB and a small source is edited
- **THEN** the large file remains in change discovery, the small source receives diagnostics, and the large file's size alone does not invalidate the baseline

#### Scenario: Large input changes
- **WHEN** the large file changes through a covered edit
- **THEN** its changed revision is detected and it receives either current diagnostics or an explicit per-file unverified status

#### Scenario: Incomplete traversal
- **WHEN** discovery exhausts its time allowance or cannot read a source
- **THEN** the partial observations survive, missing observations are not inferred as deletions, and coverage remains incomplete

### Requirement: Forward recovery without invented history

A session lacking a complete pre-edit baseline SHALL recover tracking of subsequent observations and edits while retaining the unknown historical interval. Read-only activity SHALL NOT cause all current files to be relabeled as changed. Explicit changed paths and changes relative to preserved observations SHALL still be eligible for analysis. Existing baselines SHALL NOT absorb pending edits during recovery or competing PreToolUse events.

#### Scenario: Existing session has no baseline
- **WHEN** a PostToolUse arrives without a usable pre-edit baseline
- **THEN** diagnostics report the historical gap, establish forward observation and detect a later small-file edit without claiming the historical work was checked

#### Scenario: Fresh session and parallel pre-edit hooks
- **WHEN** a fresh session observes concurrent tools before any edits
- **THEN** the earliest committed source observations are preserved and later PreToolUse does not absorb an outstanding edit

### Requirement: Non-looping infrastructure delivery and transport ownership

Native MCP and command fallback SHALL share reconciliation ownership and completed invocation receipts, independent of analysis success. Different tool calls and subagent transcripts SHALL remain distinct. Fallback SHALL handle absent or failed MCP within the existing finite hook bound. Stop SHALL inspect newer contents rather than accepting an old receipt solely because its turn identifier matches. Infrastructure-only limitations SHALL remain visible and persisted without requesting automatic continuation. Repeated actual diagnostic findings for the same source generation SHALL request at most one continuation, and stop_hook_active SHALL prevent any further automatic continuation.

#### Scenario: Concurrent native and command handlers
- **WHEN** both handlers observe the same tool completion
- **THEN** only one owns analysis and the companion recognizes its completed delivery even if the language result is unavailable

#### Scenario: MCP is absent
- **WHEN** the native MCP cannot handle the invocation
- **THEN** the installed command fallback analyzes changes or publishes explicit unverified status within its finite budget

#### Scenario: Alternating failures on repeated Stop
- **WHEN** repeated Stop or SubagentStop observes unchanged inputs and alternating baseline, scan, lock or backend failures
- **THEN** no infrastructure failure causes an automatic continuation and no clean result is invented

#### Scenario: Real defect then correction
- **WHEN** an actual diagnostic defect is discovered and later corrected
- **THEN** its findings remain visible, duplicate transport delivery cannot repeatedly continue the agent, and the correction receives current diagnostic clearance
