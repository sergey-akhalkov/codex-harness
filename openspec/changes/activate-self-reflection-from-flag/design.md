# Design

## Context

The existing managed lead host already owns the model-free journal consumer, its process containment and the visible native conversation. It currently discovers only explicitly activated bindings. Ordinary acceptance authenticates this same lead and knows the board before closing a task. See proposal.md for the user-facing gap.

## Goals / Non-Goals

Use the existing owners and native journal. Preserve the accepted-task trigger, stage checks, visible conversations, configuration routing and exact retained identities. Do not create another observer, reinterpret arbitrary task closures or bypass recovery.

## Decisions

Share activation preparation between the host and ordinary acceptance. The host reads the kit flag on its existing polling boundary and prepares on first true or a false-to-true transition. Acceptance prepares before its close, avoiding loss of a fast first event. Read-only status stays read-only. Existing explicit enable remains an idempotent compatibility operation; explicit disable remains an administrative stop until a new host or flag transition prepares the lifecycle again.

Reuse verified succession for a previous inactive owner rather than overwriting a binding. A live or unverifiable owner remains a reported conflict. Existing recovery and unknown-result rules remain authoritative.

The user confirmed that ordinary `codex` must also prepare this transport automatically. Reuse the installed launcher's already composed native command and the current managed app-server/frontend owners, rather than the legacy task runtime: that runtime requires an explicit provider base URL and rejects resume/fork. Keep the native frontend in its current terminal. Preserve the native argv, effective model/profile/configuration and session selection. Internal app-server, executor/reflection, help and noninteractive commands must not recurse into the ordinary host.

The ordinary host receives the launcher's prepared native arguments through a narrow manager entrypoint, authenticates its registered upstream and owns one app-server/frontend lifetime. Reuse existing native argument planning for backend options. Keep native new/resume/fork selection in the native frontend; observe its exact selected thread before publishing the existing process-backed lead registration. If notification observation needs the existing frontend relay, share that owner rather than inventing another transport. Publish before a first submitted turn can run tools, retire registrations on every exit path, and use the existing reflection host after registration. An already-open unmanaged conversation cannot acquire an app-server transport from a thread id; it takes the update on its next ordinary launch.

## Risks / Trade-offs

- A first close can race startup: prepare before the acceptance write, then consume the native event.
- Repeated polls can duplicate effects: retain host preparation state and the existing consumer lease; activation preserves existing checkpoints.
- A prior lead may still be alive: verify its exact retained process before succession.
- Initialization can fail: retain evidence and expose the existing host failure receipt; never claim reflection is running merely because the flag is true.

## Migration Plan

Run focused native lifecycle and acceptance checks, then deploy through the immutable global lifecycle. Verify an isolated installed consumer outside the source checkout. Existing hosts use the newly installed acceptance path; new hosts include automatic preparation. Preserve installed rollback and retained lifecycle state.
