## ADDED Requirements

### Requirement: Ordinary launch survives an upstream Codex update

When the registered upstream Codex executable or its managed `@openai/codex`
package metadata changes in place, ordinary `codex` invocation SHALL still
start that current CLI. The launcher MUST NOT require an explicit harness
update, rewrite launch registration, compile, download or start a second
session. A digest mismatch alone is not incompatibility and MUST NOT produce
a launch warning when harness enhancements still apply. A warning is allowed
only when harness enhancements cannot be applied to the current CLI, and
Codex MUST still start. Integrity checks for harness extensions, refusal of
launcher recursion and failure when the upstream executable itself is missing
SHALL remain. Check MAY report the stale registration until explicit update
refreshes it.

#### Scenario: Codex CLI updates in place
- **WHEN** a user updates the installed Codex CLI so the registered upstream executable hash or package.json digest no longer matches launch registration, and that path still names a usable Codex executable whose harness session can be prepared
- **THEN** the next `codex` invocation starts that current CLI with the supplied arguments, without an explicit-update error and without an extra compatibility warning

#### Scenario: Recursion and a missing CLI stay explicit failures
- **WHEN** launch registration points at the harness launcher itself, or the registered upstream executable is missing
- **THEN** the launcher fails explicitly without invoking an arbitrary replacement or a second session
