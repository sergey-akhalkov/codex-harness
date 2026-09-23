## MODIFIED Requirements

### Requirement: Broker generations coexist across a delivery

The shared Serena and CodeGraph brokers SHALL resolve one private location per
delivered build generation. A consumer of a newer generation MUST NOT retire a
live broker that consumers of an older generation still use; it starts or joins
its own generation's broker instead, and MUST keep the existing behavior of
joining a broker that already matches its own generation. A broker whose
consumers are all gone SHALL retire on its own idle timeout, and an explicit
maintenance retirement SHALL remain available. Records whose location no longer
exists SHALL be dropped, and locations whose generation no longer has a live
broker SHALL be pruned from the record and reusable by a newer generation, so
the record and location count stay bounded across repeated deliveries. The
location records SHALL remain readable after historical growth: a bounded
record grown by prior deliveries MUST NOT make the Serena or CodeGraph MCP
entrypoint fail before MCP initialization, and a first successful startup after
such growth SHALL rewrite the record without its dead entries.

#### Scenario: New generation starts while an older session is live

- **WHEN** a new Codex CLI session starts after a delivery while a session of
  the previous build still uses its broker
- **THEN** both sessions complete MCP initialize against their own broker, and
  neither session interrupts the other

#### Scenario: Older generation drains

- **WHEN** every consumer of a generation has finished
- **THEN** that generation's broker retires on its idle timeout while the
  delivered generation keeps serving

#### Scenario: Location record grown by many deliveries

- **WHEN** a Serena or CodeGraph MCP entrypoint starts and its location record
  contains many historical generations with no live broker, larger than the
  previous fixed read bound
- **THEN** the entrypoint completes MCP initialize and tools/list, reuses a
  free location instead of preparing an unbounded new one, and rewrites the
  record without the dead generation entries
