# Design

## Context

The executor MCP receipt currently treats its app-server process identity as
immutable. Session resume can legitimately create a new process for the same
lead thread while preserving the native thread history and executor work. The
registered endpoint is already checked for an exact live process and lead
thread; the receipt supplies run, thread and pool authority.

## Goals / Non-Goals

**Goals:** Restore an otherwise valid run after its recorded app-server exits,
without allowing a second live runtime to acquire the receipt or changing the
run's native threads or pool ownership.

**Non-Goals:** Automatically resume model work, reparent a run, reinterpret a
different lead thread as the same lead, or add a second endpoint registry or
manual record-editing command.

## Decisions

Rebind from the existing operation path. Every executor operation already
passes through exact receipt and slot verification, so performing the same
reconciliation before native effects removes the failure without adding a new
tool or state store.

Use process identity liveness, separate from endpoint authentication. The old
recorded process is reconciled only when its pid and creation time no longer
identify a running process. Query errors block the operation. The registered
endpoint keeps its existing stronger checks for live process, executable and
lead thread identity.

Apply the update under the receipt's existing lock. Re-read the receipt there,
revalidate run, lead thread, executor thread, slot, checkout, owner and the
expected old process, then replace only `lease.appServer`. Record the previous
identity as bounded `appServerRebind` evidence. An already completed rebind is
idempotent; a concurrent identity change fails closed. Re-read the registered
endpoint after the update so a replaced endpoint cannot be used as successful
reconciliation evidence.

Canonical receipt, run identity and exact slot ownership are verified before
the receipt is changed. Native thread reads or model effects happen only after
the rebound receipt has been re-read successfully.

## Risks / Trade-offs

- A deleted executable path can make exact endpoint-image validation fail even
  though the pid still exists → identity-only liveness is used solely to refuse
  rebinding; it grants no endpoint or termination authority.
- A new app-server can become available between checks → the endpoint is
  re-read after the locked receipt update, and the operation fails closed
  instead of using stale reconciliation evidence.
- Receipt persistence can fail → the operation stops before native effects and
  the existing receipt remains recoverable.

## Migration Plan

The receipt schema remains compatible: `lease.appServer` continues to record
the current exact process and `appServerRebind` is optional evidence for older
readers. Existing receipts with a live matching process are unchanged.
