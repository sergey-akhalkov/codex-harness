## 1. Adapt the philosophy

- [x] 1.1 Write global/principles-of-work.md and verify its self-contained rules cover the user's completion, quality, speed, and confirmed risk-based adaptations.
- [x] 1.2 Record upstream provenance and dispositions for every original principle in docs/principles-port.md; check the coverage against the complete source.
- [x] 1.3 Update repository guidance, the user-decision record, and documentation navigation; verify local links and remove stale statements about global principles.

## 2. Activate and verify

- [x] 2.1 Inspect current global instruction paths again and activate the canonical source; verify source/target identity and preservation of pre-existing configuration.
- [x] 2.2 Render initial Codex prompts from two separate temporary Git repositories, one with local AGENTS.md; verify the full principles and local guidance appear, then remove only the temporary probe repositories.
- [x] 2.3 Document the actual activation, update/rollback instructions, verification results, and session/override limits; verify documentation links and run strict OpenSpec validation before closing all tasks.

Task 2.2 completed: both initial-prompt probes passed (full source present exactly once; project guidance present in the second repository after the global source). After individually allowed file cleanup, the user removed the remaining temporary directories because automatic approval review blocked removal of hidden directories. The user command exited 0; a subsequent check confirmed the entire probe root is absent. The global symbolic link remains intact and its contents match the canonical source. See [activation evidence](../../../../docs/global-instructions.md).
