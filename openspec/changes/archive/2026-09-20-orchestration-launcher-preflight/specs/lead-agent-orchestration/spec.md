## ADDED Requirements

### Requirement: Launcher capability preflight and stale-build handling

Before the first executor dispatch in an activated lead session, the `team-lead` skill SHALL verify that the installed launcher implements executor dispatch by running the kit's executor usage probe (`codex-harness executor --help`) and requiring its success. When the probe reports an unsupported or unknown command, the session SHALL classify the failure as an installed launcher older than the kit instructions, report that blocker together with the kit's rebuild-and-update remedy and the registered kit source location, and continue only work whose acceptance does not depend on executors. The session SHALL NOT treat the failure as proof that executors are unavailable, substitute another profile, or bypass harness dispatch with raw native exec, TUI automation or in-session helper agents, because those paths drop the visible-conversation, steering, board and recovery contract. The manager's unknown-command diagnostic SHALL name the attempted command and state that kit instructions referencing it indicate an installed build older than the kit source, with the kit lifecycle as the remedy. An installed immutable launcher SHALL identify its recorded source identity through its own version output when that record is present.

#### Scenario: A consumer lead meets a stale launcher
- **WHEN** an activated lead session probes `codex-harness executor --help` and the installed launcher answers with an unsupported-command error
- **THEN** the session reports a stale launcher with the rebuild-and-update remedy and the kit source location, performs no executor dispatch, no profile substitution and no raw native-exec bypass, and continues only acceptance-independent work

#### Scenario: A capable launcher passes the preflight
- **WHEN** the probe prints the executor usage
- **THEN** executor dispatch proceeds through the harness command with the configured profile and the ordinary visibility, board and recovery contract

#### Scenario: The manager explains an unknown command
- **WHEN** the manager receives a command it does not implement
- **THEN** its diagnostic names that command, points at `--help`, and explains that kit instructions referencing the command mean the installed build is older than the kit source and must be rebuilt and updated through the kit lifecycle
