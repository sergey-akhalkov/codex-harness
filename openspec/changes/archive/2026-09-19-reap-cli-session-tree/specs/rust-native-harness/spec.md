## ADDED Requirements

### Requirement: Interactive CLI session process tree

Ordinary interactive Codex CLI sessions started through the harness launcher SHALL run the registered upstream executable inside a Windows Job that kills remaining members when the last job handle closes. Containment SHALL be established before the upstream image executes. The session job SHALL NOT apply the helper memory or CPU caps used for MCP, indexer and probe workers. Independently started kit services, including the xAI shim and shared MCP brokers, SHALL remain outside that session job. Interactive standard streams and the calling console SHALL be inherited so argument, Unicode, cwd, stdin, stdout, stderr and exit-code contracts stay unchanged. The launcher SHALL wait for the session root to exit with no execution deadline, then reap surviving session descendants. Cleanup MUST target only that owned tree. Closing the wrapper or the console SHALL also reclaim the tree. The kit SHALL NOT hunt processes by name or PID alone, SHALL NOT scan the computer for closed sessions, and SHALL NOT leave a detached scavenger running after the launcher returns.

#### Scenario: Hidden helper outlives the CLI today
- **WHEN** an ordinary `codex` session started through the harness launcher spawns a detached helper and then the CLI root exits
- **THEN** the helper is reclaimed with the session job, a separately started kit service keeps running, and the launcher returns the CLI exit code

#### Scenario: Wrapper or console dies first
- **WHEN** the harness launcher process or its console is terminated while session descendants are still running
- **THEN** those owned descendants terminate and unrelated processes remain

#### Scenario: Interactive streams stay attached
- **WHEN** a user starts `codex` from an existing terminal through the harness launcher
- **THEN** the upstream CLI inherits that console and standard streams and is not attached to NUL
