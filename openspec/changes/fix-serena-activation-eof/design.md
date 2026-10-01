## Context

See proposal.md. Native crash evidence shows a worker exit during activation and allocation failures in adjacent worker startups. The worker Job allows 4 GiB, but the service bootstrap creates an outer 2 GiB Job containing every worker. Windows accounts child jobs against the parent, so the configured worker allowance is unreachable under that aggregate cap. The current retry allowlist separately excludes activation.

## Goals / Non-Goals

Restore the existing worker allowance and bounded activation recovery. Preserve worker count, per-worker memory, CPU, process ownership, language coverage and non-replayed edits. Do not remove resource containment or infer that every possible upstream crash is repaired.

## Decisions

- Keep the bounded bootstrap Job, then set its aggregate memory allowance before starting workers to the existing 2 GiB service allowance plus configured worker capacity times the existing 4 GiB worker allowance. Derive it from the validated resource policy, rather than introducing an unrelated fixed ceiling. This is a commit ceiling, not reserved physical memory.
- Preserve all other Job flags while updating only its memory limit; verify kernel readback. Record that readback once in the owned service log, and include native worker exit and local memory observations in transport errors.
- Add activation to the existing one-retry allowlist. The pool already resolves the requested project and commits the client route only on success. Retain the original deadline and never replay source edits.
- Preserve closed-writer errors as typed broken-pipe failures. Native regression execution returned Windows error 232 wrapped as an unclassified string, which bypassed recovery even though reader EOF was already typed. Cover both pipe directions with actual owned endpoints.
- Reuse the existing native process fixtures and real Serena transport relay. Cover native memory enforcement, actual broker configuration, response loss after activation, route correctness, repeated failure and uncertain edits.

## Risks / Trade-offs

- The corrected aggregate permits the already configured workers to use more memory than the accidental 2 GiB cap; unchanged per-worker limits and bounded pool admission still constrain usage.
- A repeatable upstream crash remains an error after one retry. The original local project must complete semantic navigation before repair acceptance.

## Migration Plan

Run baseline and candidate regression checks, native checks and real semantic acceptance. Deploy through the existing immutable lifecycle. Existing loaded proxies require a new session to adopt the new binary; preserve unrelated active work.
