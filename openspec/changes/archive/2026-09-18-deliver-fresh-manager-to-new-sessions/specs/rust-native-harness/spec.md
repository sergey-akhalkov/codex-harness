## ADDED Requirements

### Requirement: Fresh manager delivery to new sessions

Installing or updating the native manager SHALL deliver a fresher published
build by adding its immutable files and moving the stable links that new
consumers resolve, and MUST NOT replace, delete or rewrite a build file that a
running session may hold. The delivered build SHALL be an integrity-verified
published build: the explicit `--build` when the operator supplies one,
otherwise the freshest verified build published from the selected source in
the owned state of the running manager. When the running manager does not
belong to an owned state, or no verified build matches the selected source,
the explicit build remains required and the failure is reported before any
registration changes. A Codex CLI session started before delivery SHALL keep
its already-resolved manager and MUST NOT be interrupted; a session started
after delivery SHALL resolve the freshly delivered manager through the stable
links. Selection MUST stay explicit: ordinary launch performs no build,
download or registration mutation, and a build that merely exists without an
explicit Install/Update MUST NOT become the consumer's manager.

#### Scenario: Delivery while an earlier manager still runs

- **WHEN** Install/Update delivers a newer verified build while a session still
  runs the previous manager
- **THEN** the operation succeeds without touching the previous build file, the
  running session continues on its manager, and a new consumer resolves the
  delivered build through the stable manager link

#### Scenario: Scoped update from an older manager

- **WHEN** a scoped update is run from, or refers to, an older manager while the
  owned state already holds a fresher verified build
- **THEN** new sessions resolve the freshest delivered build, and the update
  does not pin the older manager for later consumers

#### Scenario: Unverified or foreign candidate

- **WHEN** the newest directory under an owned state has a missing, altered or
  unverifiable record or binary
- **THEN** it is not delivered, the previous delivered manager stays in place
  and the failure is reported explicitly

### Requirement: Broker generations coexist across a delivery

The shared Serena and CodeGraph brokers SHALL resolve one private location per
delivered build generation. A consumer of a newer generation MUST NOT retire a
live broker that consumers of an older generation still use; it starts or joins
its own generation's broker instead, and MUST keep the existing behavior of
joining a broker that already matches its own generation. A broker whose
consumers are all gone SHALL retire on its own idle timeout, and an explicit
maintenance retirement SHALL remain available. Records whose location no longer
exists SHALL be dropped so the generation list stays bounded.

#### Scenario: New generation starts while an older session is live

- **WHEN** a new Codex CLI session starts after a delivery while a session of
  the previous build still uses its broker
- **THEN** both sessions complete MCP initialize against their own broker, and
  neither session interrupts the other

#### Scenario: Older generation drains

- **WHEN** every consumer of a generation has finished
- **THEN** that generation's broker retires on its idle timeout while the
  delivered generation keeps serving
