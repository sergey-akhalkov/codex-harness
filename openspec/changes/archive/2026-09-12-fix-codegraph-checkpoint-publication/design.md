## Context

See [proposal](proposal.md). A fresh native build restores MCP startup. The
installed three-project consumer scenario then reproduced automatic refresh
failure twice; a live broker status identified Windows error 5. A native
held-reader regression reproduced that error while rotating the committed
checkpoint, then passed with bounded rotation. Committed checkpoint inspection
and publication overlap in the installed scenario. The corrected published
three-project scenario and final installed Codex scenario both passed.

## Goals / Non-Goals

Preserve the directory-pair checkpoint format, ownership checks, saved data,
single indexing admission and existing deadline. Keep stale-build enforcement
and the pinned upstream package. No new storage service or dependency.

## Decisions

Retain the native held-reader assertion that failed on the original source.
Retry only the checkpoint directory rotation on Windows errors 5, 32 or 33,
with 10 ms pauses, a two-second ceiling and the shorter operation deadline.
Check cancellation before each attempt. Persistent errors include operation
context. Changing ACLs or weakening ownership checks does not address an open
handle; retrying a whole indexing episode would repeat completed work. The
existing directory-pair recovery continues to own interrupted publication.

Windows sharing rules require compatible open handles for rename/delete:
[Microsoft file rename guidance](https://devblogs.microsoft.com/oldnewthing/20211022-00/?p=105822)
and [MoveFileEx contract](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexa).
The reduced native test verifies the applicable sharing behavior locally.

## Risks / Trade-offs

- A persistent lock or actual ACL denial must remain an explicit failure;
  waiting must stop on cancellation/deadline and retain the previous checkpoint.
- A consumer regression can race publication itself; retain original execution
  evidence and require the installed scenario after the focused native checks.

## Migration Plan

Build through the existing immutable native build command, qualify the installed
package, and update only the owned CodeGraph registration and inventory. Keep
the old build and local configuration snapshots for recovery. Verify fresh
native Codex consumers outside the checkout; existing sessions need restart.
