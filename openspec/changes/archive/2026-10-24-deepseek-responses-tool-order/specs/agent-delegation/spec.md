## ADDED Requirements

### Requirement: DeepSeek executor transport preserves tool-call pairing

DeepSeek executor requests SHALL use an owned loopback Responses transport that
keeps each tool-call batch ahead of its matching outputs, so an inserted
developer, user or reasoning item cannot permanently reject a thread. The
transport SHALL preserve item content and authorization, and SHALL leave other
provider routes unchanged.

#### Scenario: An internal item interrupts one tool exchange

- **WHEN** a DeepSeek request contains a tool call, an inserted non-tool item,
  and the matching output later in the same input
- **THEN** the transport forwards the call and its output together before the
  inserted item
- **AND** the same saved thread can continue without rewriting its rollout

#### Scenario: An inserted item follows a parallel call batch

- **WHEN** multiple calls from one batch are followed by an inserted non-tool
  item and their matching outputs
- **THEN** the transport preserves call order and output pairing ahead of the
  inserted item
- **AND** no request item is dropped, duplicated or synthesized

#### Scenario: An already valid request is sent unchanged

- **WHEN** a DeepSeek request already has a valid call/output ordering
- **THEN** the transport forwards the same semantic input without moving items

#### Scenario: Another provider keeps its existing route

- **WHEN** a request does not use the DeepSeek loopback route
- **THEN** DeepSeek ordering repair is not applied
- **AND** the existing xAI request adaptations and lifecycle remain available

#### Scenario: Provider metadata request has no body

- **WHEN** the client requests DeepSeek model metadata without a request body
- **THEN** the transport forwards the request method and path suffix without
  inventing a body or rejecting the request for a missing length
