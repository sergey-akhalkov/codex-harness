## Context

See proposal.md for why. Windows Terminal's default `closeOnExit` is graceful: the tab closes only when its root process exits 0. The executor tab's root process is `executor run --file RECEIPT`. A recorded failure returns a non-zero code, so the tab stays open. Dispatch does not wait on that process; `executor watch` reads the receipt. Stop already refuses to send a terminal command, because a close command can hit the wrong tab.

The 1 MiB bound is the control transport's own websocket limit. `thread/read includeTurns` asks for the whole thread. After a long run that response can exceed the limit even though each streamed `item/completed` already fitted. The host currently treats that read error as fatal and terminates the child tree.

## Goals / Non-Goals

**Goals:**

- Close the executor tab on any host exit the tab process itself can finish, without a terminal command.
- Keep the receipt's state and exit code as the run outcome.
- Keep a completed turn when the only failure is an oversized full-thread read.

**Non-Goals:**

- Installing or editing Windows Terminal settings.
- Changing owned-console exit codes.
- Replacing `thread/read` on the successful path.
- Making a killed or panicked host close its tab. That still depends on the terminal's close-on-exit policy; stop continues to end the host and verify the recorded tab.

## Decisions

### Exit 0 from the tab host, not a terminal close command

The tab command gains a flag used only by Windows Terminal dispatch. After the hosted session returns, that process exits 0. Graceful close-on-exit then closes the tab. The receipt is written before that exit and keeps the real state and exit code. An error is printed before the process exits 0, so the cause is not dropped from the host output; the lead's durable copy is the receipt.

Alternatives:

- A Windows Terminal profile with `closeOnExit: always` would also close panics and external kills, but it requires a settings fragment the running terminal may not load, and a caller-supplied `--terminal-profile` would drop it. Rejected as the mechanism the observed failure depends on.
- `wt` close-tab can close the wrong tab. The stop path already forbids that. Rejected.
- Leaving the tab open preserves the previous inspection choice. The user superseded it: the receipt is the inspection surface.

### Recover the already delivered message instead of raising the transport limit

On a completed turn, if `thread/read` fails because the response exceeds the transport limit, use the last nonempty `item/completed` agent message already received on that turn. That event is the same item the thread read would have returned, and it already fitted in one record. If no such message was delivered, record an output defect that names the size limit and end the child normally. Any other read failure still fails the host and terminates the tree.

Raising the 1 MiB client limit would only move the same failure to the next longer thread. A paginated `thread/turns/list` read is the later migration already noted for this driver; it is not required to stop this failure, because the completed item is already in hand. Switching the happy path to pagination would change every existing final-message test without fixing a case the stream did not already deliver.

## Risks / Trade-offs

- [Tab process exit 0 is not the run outcome] → Watch and the receipt keep the real exit code. Help text says the tab host exits 0 so the tab closes.
- [A kill or panic still leaves the tab] → Stop still ends the host and reports a tab that did not close. This change does not claim to close a process that never returns.
- [A huge final message that never arrived as `item/completed` becomes a defect, not a success] → No fabricated completion. The cause names the size limit.
- [The tab closes before a person can read the on-screen error] → That is the requested outcome. The receipt, detail file and control log keep the cause.

## Migration Plan

Rebuild and install the launcher through the existing kit lifecycle. Older installed launchers keep the previous tab behavior until that update. No receipt schema change. Rollback is reverting the tab flag and the size-limit recovery.
