## Context

Commit f4cfe68 moved exact-session resume onto the host's managed control
conversation. `ControlConnection` had carried a one-mebibyte
`max_message_size`/`max_frame_size`/`max_write_buffer_size` bound since the
transport was written, and the warning relay added in c5b5e02 copied the same
bound onto the frontend hop. Nothing in the app-server contract caps record
size; thread state grows with the conversation, and 1 MiB is far below a real
long session.

## Decision

Remove the size caps entirely on both managed hops rather than raising them to
another guess that the next long session crosses. Both peers of each hop are
owned localhost processes (the host connects to its own app-server child; the
relay fronts the native frontend it spawned with the conversation's capability
token), so an unbounded record is not a remote attack surface. All time bounds
stay: connect, read poll, socket write and relay shutdown remain bounded, so a
hung peer still cannot wedge the host.

With no size refusal, the delivered-message fallback for an oversized
final-message read has no trigger left; a full-thread read that fails for any
reason still fails the host honestly. The relay's write-failure handling
(`undelivered` while the diagnostic stays queued) remains in the code, but its
only deterministic test trigger was the removed cap, so the large-diagnostic
test now pins native delivery instead; that coverage limit is recorded in the
tasks.

## Risks

Memory now grows with the largest record a peer actually sends. That is the
app-server's own contract on the control hop and the native frontend's own
output on the relay hop; the host already ran the same data through the
frontend. The canned test double keeps its own 16 MiB sanity bound so a broken
double cannot OOM the test process.
