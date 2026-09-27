## ADDED Requirements

### Requirement: Independent Serena workers make concurrent progress

Requests for different compatible worker identities SHALL progress independently when configured worker capacity is available. A slow request, worker startup or worker retirement for one project SHALL NOT serialize all requests to already available independent workers. Requests using the same worker SHALL remain serialized. Mutable routing operations of the same client SHALL remain ordered, and every response SHALL preserve the originating client, request id, canonical root and applicable configuration. Concurrency SHALL NOT broaden automatic retry of uncertain mutations.

#### Scenario: Two independent requests overlap
- **WHEN** one worker is held inside a request and a second client submits a request to another available worker
- **THEN** the second worker can enter and complete its request before the first worker is released

#### Scenario: One worker receives concurrent clients
- **WHEN** compatible clients submit concurrent requests sharing one worker
- **THEN** that worker processes one request at a time and each client receives the correct response

#### Scenario: Slow startup beside a warm project
- **WHEN** one project starts a worker while another project has an available worker and capacity permits both
- **THEN** the warm project continues answering requests without waiting for unrelated startup

#### Scenario: One client changes project selection
- **WHEN** a client activates or removes a project and submits a subsequent project-sensitive request
- **THEN** the subsequent request observes the ordered routing outcome and other clients retain their own selections and results

### Requirement: Concurrent Serena admission preserves bounded ownership

Configured worker capacity SHALL include active, starting and retiring workers until their ownership has been released. Compatible concurrent admissions SHALL share one startup. In-flight and reserved work SHALL NOT be evicted. Excess roots SHALL remain supported through bounded admission and idle-worker replacement; capacity exhaustion SHALL NOT silently reduce executor slots, increase worker limits or kill an unrelated request. Waiting, startup, execution and cleanup SHALL respect their existing request/ownership deadlines and cancellation contracts. A request that expires before admission SHALL NOT execute later. Failed startup and interrupted shutdown SHALL preserve recoverable ownership state without an untracked process tree.

#### Scenario: All worker slots are busy
- **WHEN** an additional project requests service while every configured slot is occupied by protected work
- **THEN** it waits within its request budget or receives an explicit capacity/deadline outcome, no protected worker is evicted and the configured worker bound is preserved

#### Scenario: An idle worker can be replaced
- **WHEN** capacity is full and an idle worker is selected for replacement
- **THEN** its owned process tree is retired before replacement exceeds the capacity bound, while unrelated available workers continue serving

#### Scenario: Duplicate cold requests
- **WHEN** multiple clients concurrently request the same compatible identity before its worker is ready
- **THEN** they share a single startup outcome rather than creating duplicate workers

#### Scenario: Cancellation while queued
- **WHEN** a caller cancels or reaches its deadline before obtaining a worker
- **THEN** its pending admission is released, its request never executes afterward and other clients remain usable

#### Scenario: Configuration changes during a request
- **WHEN** a worker's configuration identity becomes obsolete while admitted work is running
- **THEN** the replacement cannot reuse mismatched state or evict that in-flight work, and old and replacement generations remain within the capacity/ownership contract

### Requirement: Serena pool status supports evidence-based capacity decisions

The existing local service-status evidence SHALL expose bounded hit, cold-start, eviction and startup-failure counts, active/reserved/idle worker counts and queue/start/request durations with their observation interval or reset identity. Memory measurements SHALL distinguish actual observations from configured limits and unavailable data. Routine tool results SHALL NOT be expanded into continuous metrics reports, and observations SHALL NOT imply token savings. The delivered worker and executor defaults SHALL remain unchanged by this improvement.

#### Scenario: Four roots cycle through a three-worker pool
- **WHEN** four independent roots sequentially use a configured three-worker pool and revisit an evicted root
- **THEN** status identifies the cold start and eviction without claiming all roots stayed warm, and no additional worker is silently admitted

#### Scenario: Broker restarts
- **WHEN** counters reset after service restart
- **THEN** status identifies the new observation interval instead of presenting reset values as lifetime totals
