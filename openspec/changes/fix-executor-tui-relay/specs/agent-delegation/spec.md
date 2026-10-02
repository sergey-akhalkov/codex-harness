## ADDED Requirements

### Requirement: Responsive executor frontend transport

Managed executor presentation SHALL forward ready conversation traffic without waiting for user input between server events or waiting for server output between client requests. It SHALL preserve payloads and ordering, bounded resource use, authenticated connection boundaries, exact-thread warnings and reconnect to the same running conversation without replaying model or tool requests.

#### Scenario: Streaming while the user is idle
- **WHEN** the app-server emits a burst of progress events while the frontend sends no input
- **THEN** the frontend receives the ordered burst without a per-event idle-peer delay and heartbeat traffic continues

#### Scenario: Requests while the server is idle
- **WHEN** the frontend sends a burst of requests before the server responds
- **THEN** all requests reach the server in order without a per-request idle-peer delay

#### Scenario: Frontend reconnects during a running assignment
- **WHEN** a frontend disconnects and authenticates a new connection to the same live conversation
- **THEN** it can receive current progress without restarting the assignment, replaying requests or changing model binding
