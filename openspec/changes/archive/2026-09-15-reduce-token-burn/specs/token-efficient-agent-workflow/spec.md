## ADDED Requirements

### Requirement: Per-model default reasoning effort

The launcher SHALL apply a model-specific default reasoning effort to ordinary
session starts that select no explicit effort: "max" for zai/glm-5.3 and
"xhigh" for xai/grok-4.6 and the Astra family. An explicit effort argument, a
profile, a remote route or the harness effort selector SHALL take precedence
and leave the effective selection unchanged. Unknown or unmapped models SHALL
receive no injected effort. The effective effort SHALL remain observable in
native session evidence.

#### Scenario: Grok starts without an explicit effort
- **WHEN** a session selects xai/grok-4.6 without any effort argument
- **THEN** the session runs at xhigh instead of inheriting an unrelated machine-level effort

#### Scenario: GLM keeps its heavier default
- **WHEN** a session selects zai/glm-5.3 without any effort argument
- **THEN** the session runs at max

#### Scenario: An explicit effort wins
- **WHEN** arguments contain an explicit effort setting, a profile, a remote route or the harness effort selector
- **THEN** no per-model default is injected and the explicit selection applies

#### Scenario: Unmapped model
- **WHEN** the selected model has no mapping
- **THEN** the native effort configuration is passed through unchanged

### Requirement: Session-lifecycle economy

The portable workflow guidance SHALL direct agents to start a new session for a
new topic, avoid resuming very long threads for small follow-ups, prefer forking
with a concise handoff over continuing marathon threads, and give children
concise briefs instead of full parent history. The guidance SHALL be advisory
working practice grounded in measured token evidence, not a forced scheduler or
turn limit, and SHALL NOT weaken required acceptance checks or task continuity.

#### Scenario: Small follow-up after a marathon thread
- **WHEN** a tiny question arrives long after a very large session finished
- **THEN** the agent starts a fresh session or forks with a concise handoff instead of repaying the entire history

#### Scenario: Delegating independent work
- **WHEN** a child agent receives a bounded assignment
- **THEN** the brief carries objective, inputs, ownership and checks without the parent's full transcript

### Requirement: Lean default connector surface

Portable defaults SHALL disable the native Apps connector feature. A
machine-local true value SHALL take precedence and re-enable Apps for that
machine. Plugin installation SHALL remain an explicit per-machine opt-in, and
the kit SHALL NOT require any installed app or plugin for its accepted
operation or checks.

#### Scenario: Fresh consumer session
- **WHEN** an ordinary session starts with portable defaults and no machine override
- **THEN** Apps connector tools are absent from the session tool surface

#### Scenario: Machine-local re-enable
- **WHEN** the machine configuration explicitly enables the Apps feature
- **THEN** the local value wins and Apps tools return without editing portable sources

#### Scenario: Kit operation without connectors
- **WHEN** installation, update or checks run with Apps disabled and no plugins installed
- **THEN** every accepted kit operation still completes
