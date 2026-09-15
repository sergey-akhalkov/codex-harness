## ADDED Requirements

### Requirement: Ordinary sessions receive live shared defaults

An ordinary local `codex` session SHALL receive the live checkout shared defaults that the kit declares for new tasks, including developer instructions and the accepted experimental context-management trial, together with applicable local overrides. A source-stale or otherwise non-runtime native build MUST NOT silently omit those defaults. If live injection cannot be established, the launcher SHALL report a degraded session without kit shared defaults, Check SHALL record the same failure, and that session MUST NOT be treated as a successful kit consumer. Explicit user profile or `-c` overrides remain effective. Management, help and version commands MAY keep their native invocation without session defaults.

#### Scenario: Native build identity is source-stale
- **WHEN** the recorded native manager no longer matches the current checkout source and an ordinary local session starts
- **THEN** the session still receives live shared defaults, or the launcher reports the degraded fallback and Check flags missing defaults

#### Scenario: Fresh consumer prompt is inspected
- **WHEN** `codex debug prompt-input` runs from an outside repository through the installed ordinary entry point
- **THEN** the dump includes the current developer-instruction marker from shared defaults, unless the degraded fallback was explicitly reported

